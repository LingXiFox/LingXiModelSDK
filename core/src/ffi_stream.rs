//! Incremental streaming across the C ABI.
//!
//! `lingxi_stream_open` returns as soon as the upstream response headers
//! arrive; `lingxi_stream_next` then blocks for exactly one event, so foreign
//! languages observe tokens while the provider response is still open.
//! `lingxi_stream_cancel` wakes a blocked reader from any thread and
//! `lingxi_stream_close` releases the connection.
//!
//! Handles are opaque monotonic ids handed out as pointers, never real
//! allocations. Every entry point resolves the id through a process-wide
//! registry, so NULL, released and forged handles are rejected without ever
//! dereferencing caller memory. That makes double-close, use-after-close and
//! close-while-reading safe: an in-flight read keeps its own `Arc`, and the
//! stream itself is only dropped once the last reader returns.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::Notify;

use crate::errors::NativeError;
use crate::lmp::client::LmpStreamHandle;
use crate::lmp::types::LmpRequest;
use crate::native::{NativeClient, NativeRequestConfig};
use crate::registry::NativeProtocol;
use crate::stream::EventStreamHandle;

fn c_str(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr) }.to_str().ok().map(ToString::to_string)
}

fn to_c_json(value: Value) -> *mut c_char {
    let text = value.to_string();
    match CString::new(text) {
        Ok(text) => text.into_raw(),
        Err(_) => CString::new(r#"{"ok":false,"error":"interior nul byte"}"#)
            .expect("static JSON has no interior nul")
            .into_raw(),
    }
}

fn error_envelope(code: &str, message: impl Into<String>) -> *mut c_char {
    to_c_json(json!({"ok": false, "code": code, "error": message.into()}))
}

/// Live handles by id. Ids are monotonically increasing, so a stale handle can
/// never alias a freshly opened stream.
fn handles() -> &'static Mutex<HashMap<u64, Arc<LingxiStream>>> {
    static HANDLES: OnceLock<Mutex<HashMap<u64, Arc<LingxiStream>>>> = OnceLock::new();
    HANDLES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn open_error_slot() -> &'static Mutex<Option<String>> {
    static SLOT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn set_open_error(message: impl Into<String>) {
    if let Ok(mut slot) = open_error_slot().lock() {
        *slot = Some(message.into());
    }
}

fn next_handle_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Stable machine code for a native failure, prefixed into the message so the
/// caller can classify without a second out-parameter.
fn code_and_message(err: &NativeError) -> String {
    let code = err.code();
    format!("{code}: {err}")
}

enum StreamBody {
    Native(EventStreamHandle),
    Unified(LmpStreamHandle),
}

impl StreamBody {
    async fn next(&mut self) -> Result<Option<Value>, String> {
        match self {
            Self::Native(handle) => match handle.next_event().await {
                Ok(Some(event)) => Ok(Some(json!({
                    "event": event.event,
                    "data": event.data,
                    "id": event.id,
                }))),
                Ok(None) => Ok(None),
                Err(err) => Err(code_and_message(&err)),
            },
            Self::Unified(handle) => match handle.next_event().await {
                Ok(Some(event)) => Ok(Some(json!(event))),
                Ok(None) => Ok(None),
                Err(err) => Err(format!("{}: {err}", err.code())),
            },
        }
    }
}

/// Opaque stream handle. Handed to C as a pointer-sized token; it is never
/// dereferenced on the C side and never freed while a reader is inside it.
pub struct LingxiStream {
    runtime: tokio::runtime::Runtime,
    body: Mutex<Option<StreamBody>>,
    cancel: Arc<AtomicBool>,
    notify: Arc<Notify>,
    last_error: Mutex<Option<String>>,
    closed: AtomicBool,
    unified: bool,
    id: u64,
}

impl LingxiStream {
    fn record_error(&self, message: String) {
        if let Ok(mut slot) = self.last_error.lock() {
            *slot = Some(message);
        }
    }

    fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.notify.notify_one();
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamOpenRequest {
    protocol: String,
    base_url: String,
    api_key: Option<String>,
    model: Option<String>,
    payload: Value,
    #[serde(default)]
    headers: HashMap<String, String>,
    timeout_ms: Option<u64>,
    /// Idle timeout while waiting for the next upstream byte.
    idle_timeout_ms: Option<u64>,
    #[serde(default)]
    unified: bool,
}

/// Opens an incremental stream.
///
/// The request JSON matches `lingxi_runtime_request` plus optional
/// `idleTimeoutMs`. Returns an opaque handle, or NULL on failure; when NULL is
/// returned the reason is available from `lingxi_stream_open_error()`.
#[no_mangle]
pub extern "C" fn lingxi_stream_open(request_json_utf8: *const c_char) -> *mut LingxiStream {
    let open = || -> Result<(LingxiStream, u64), String> {
        let raw = c_str(request_json_utf8)
            .ok_or_else(|| "null or invalid stream request".to_string())?;
        let request: StreamOpenRequest = serde_json::from_str(&raw)
            .map_err(|e| format!("invalid stream request JSON: {e}"))?;
        let protocol = NativeProtocol::parse_str(&request.protocol)
            .ok_or_else(|| format!("unknown protocol: {}", request.protocol))?;
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err("blocking C ABI must run outside an async runtime".into());
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|e| format!("could not start Rust HTTP runtime: {e}"))?;
        let idle = Duration::from_millis(request.idle_timeout_ms.unwrap_or(120_000));
        let opened = runtime.block_on(async {
            if request.unified {
                if !request.headers.is_empty() {
                    return Err("unified stream does not accept custom HTTP headers".to_string());
                }
                let mut payload: LmpRequest = serde_json::from_value(request.payload)
                    .map_err(|e| format!("invalid unified request payload: {e}"))?;
                payload.timeout_ms = request.timeout_ms.or(payload.timeout_ms);
                let client = crate::lmp::UnifiedClient::new();
                let handle = client
                    .open_stream(protocol, &request.base_url, request.api_key, &payload)
                    .await
                    .map_err(|e| format!("{}: {e}", e.code()))?;
                let cancel = handle.cancellation();
                let notify = handle.cancellation_notify();
                Ok((StreamBody::Unified(handle), cancel, notify))
            } else {
                let mut config = NativeRequestConfig::new(request.base_url, request.api_key);
                config.headers = request.headers;
                config.stream_idle_timeout = idle;
                if let Some(timeout) = request.timeout_ms {
                    config.timeout = Duration::from_millis(timeout);
                }
                let client = NativeClient::new();
                let handle = client
                    .open_stream_events(protocol, &config, request.model.as_deref(), &request.payload)
                    .await
                    .map_err(|e| code_and_message(&e))?;
                let cancel = handle.cancellation();
                let notify = handle.cancellation_notify();
                Ok((StreamBody::Native(handle), cancel, notify))
            }
        })?;
        let (body, cancel, notify) = opened;
        Ok((
            LingxiStream {
                runtime,
                body: Mutex::new(Some(body)),
                cancel,
                notify,
                last_error: Mutex::new(None),
                closed: AtomicBool::new(false),
                unified: request.unified,
                id: 0,
            },
            next_handle_id(),
        ))
    };

    match open() {
        Ok((mut stream, id)) => {
            stream.id = id;
            if let Ok(mut live) = handles().lock() {
                live.insert(id, Arc::new(stream));
            } else {
                set_open_error("stream registry unavailable");
                return std::ptr::null_mut();
            }
            id as usize as *mut LingxiStream
        }
        Err(message) => {
            set_open_error(message);
            std::ptr::null_mut()
        }
    }
}

/// Reason the last `lingxi_stream_open` returned NULL.
#[no_mangle]
pub extern "C" fn lingxi_stream_open_error() -> *mut c_char {
    let message = open_error_slot()
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
        .unwrap_or_else(|| "unknown stream error".to_string());
    error_envelope("open_failed", message)
}

/// Resolves a handle token to its live stream, keeping it alive for the call.
fn resolve(ptr: *mut LingxiStream) -> Option<Arc<LingxiStream>> {
    if ptr.is_null() {
        return None;
    }
    let id = ptr as usize as u64;
    let live = handles().lock().ok()?;
    live.get(&id).cloned()
}

/// Reads the next event. `timeoutMs = 0` waits indefinitely.
///
/// Clean end: `{"ok":true,"done":true}`. Event: `{"ok":true,"event":{...}}`
/// (native framing `{event,data,id}` or an LMP event when `unified` was set).
/// Failure: `{"ok":false,"code":"...","error":"..."}`; the upstream connection
/// is released and later calls keep reporting the recorded failure.
#[no_mangle]
pub extern "C" fn lingxi_stream_next(ptr: *mut LingxiStream, timeout_ms: u64) -> *mut c_char {
    let Some(stream) = resolve(ptr) else {
        return error_envelope("invalid_handle", "null or released stream handle");
    };
    if stream.closed.load(Ordering::SeqCst) {
        return error_envelope("invalid_handle", "stream handle is closed");
    }
    if tokio::runtime::Handle::try_current().is_ok() {
        return error_envelope(
            "invalid_context",
            "blocking C ABI must run outside an async runtime",
        );
    }
    let mut guard = match stream.body.lock() {
        Ok(guard) => guard,
        Err(_) => return error_envelope("invalid_handle", "stream handle poisoned"),
    };
    let Some(body) = guard.as_mut() else {
        return to_c_json(json!({"ok": true, "done": true}));
    };
    let deadline = (timeout_ms > 0).then(|| Duration::from_millis(timeout_ms));
    let outcome = stream.runtime.block_on(async {
        match deadline {
            Some(limit) => match tokio::time::timeout(limit, body.next()).await {
                Ok(result) => result,
                Err(_) => Err(format!("timeout: no event within {timeout_ms} ms")),
            },
            None => body.next().await,
        }
    });
    match outcome {
        Ok(Some(event)) => to_c_json(json!({"ok": true, "event": event})),
        Ok(None) => to_c_json(json!({"ok": true, "done": true})),
        Err(message) => {
            stream.record_error(message.clone());
            // Release the upstream connection immediately on failure.
            *guard = None;
            let code = message.split(':').next().unwrap_or("error").to_string();
            error_envelope(&code, message)
        }
    }
}

/// Requests cancellation. Safe from any thread, including while `next` blocks.
/// Also safe on NULL and on released handles.
#[no_mangle]
pub extern "C" fn lingxi_stream_cancel(ptr: *mut LingxiStream) {
    if let Some(stream) = resolve(ptr) {
        stream.request_cancel();
    }
}

/// Releases the handle and aborts the stream. Idempotent and NULL-safe.
#[no_mangle]
pub extern "C" fn lingxi_stream_close(ptr: *mut LingxiStream) {
    if ptr.is_null() {
        return;
    }
    let id = ptr as usize as u64;
    let entry = match handles().lock() {
        Ok(mut live) => live.remove(&id),
        Err(_) => None,
    };
    let Some(stream) = entry else {
        return;
    };
    stream.closed.store(true, Ordering::SeqCst);
    stream.request_cancel();
    // The last reader may still hold an `Arc`; the stream (and its runtime)
    // is dropped when that reader returns.
    drop(stream);
}

/// Last error recorded for this handle, plus diagnostics.
#[no_mangle]
pub extern "C" fn lingxi_stream_last_error(ptr: *mut LingxiStream) -> *mut c_char {
    let Some(stream) = resolve(ptr) else {
        return error_envelope("invalid_handle", "null or released stream handle");
    };
    let message = stream
        .last_error
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
        .unwrap_or_default();
    to_c_json(json!({
        "ok": true,
        "error": message,
        "closed": stream.closed.load(Ordering::SeqCst),
        "unified": stream.unified,
        "id": stream.id,
    }))
}
