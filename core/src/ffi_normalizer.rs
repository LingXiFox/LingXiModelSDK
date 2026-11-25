//! Incremental LMP normalization across the C ABI.
//!
//! `lingxi_lmp_normalize_stream_events` needs the whole event array, which
//! forces foreign callers to buffer a complete upstream response. The session
//! handle below lets a host language feed one SSE/NDJSON frame at a time and
//! receive the resulting LMP events immediately, so Unified streaming stays
//! incremental even when the HTTP transport lives in the host language.
//!
//! Sessions are opaque monotonic ids resolved through a process-wide registry.
//! They hold no sockets and no runtime, so they are cheap and never block.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{json, Value};

use crate::lmp::adapters::StreamNormalizer;
use crate::registry::NativeProtocol;
use crate::sse::SseEvent;

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

/// One incremental normalization session.
pub struct LingxiNormalizer {
    normalizer: Mutex<Option<StreamNormalizer>>,
    protocol: NativeProtocol,
    finished: AtomicBool,
}

fn sessions() -> &'static Mutex<HashMap<u64, Arc<LingxiNormalizer>>> {
    static SESSIONS: OnceLock<Mutex<HashMap<u64, Arc<LingxiNormalizer>>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn resolve(ptr: *mut LingxiNormalizer) -> Option<Arc<LingxiNormalizer>> {
    if ptr.is_null() {
        return None;
    }
    let id = ptr as usize as u64;
    sessions().lock().ok()?.get(&id).cloned()
}

/// Opens a normalization session for one protocol.
///
/// Returns NULL for an unknown protocol; the reason is available from
/// `lingxi_normalizer_open_error()`.
#[no_mangle]
pub extern "C" fn lingxi_lmp_normalizer_open(protocol_utf8: *const c_char) -> *mut LingxiNormalizer {
    static LAST_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    let record = |message: String| {
        if let Ok(mut slot) = LAST_ERROR.get_or_init(|| Mutex::new(None)).lock() {
            *slot = Some(message);
        }
    };
    let Some(raw) = c_str(protocol_utf8) else {
        record("null or invalid protocol".to_string());
        return std::ptr::null_mut();
    };
    let Some(protocol) = NativeProtocol::parse_str(&raw) else {
        record(format!("unknown protocol: {raw}"));
        return std::ptr::null_mut();
    };
    let id = next_id();
    let session = Arc::new(LingxiNormalizer {
        normalizer: Mutex::new(Some(StreamNormalizer::new(protocol))),
        protocol,
        finished: AtomicBool::new(false),
    });
    match sessions().lock() {
        Ok(mut live) => {
            live.insert(id, session);
            if let Ok(mut slot) = LAST_ERROR.get_or_init(|| Mutex::new(None)).lock() {
                *slot = None;
            }
            id as usize as *mut LingxiNormalizer
        }
        Err(_) => {
            record("normalizer registry unavailable".to_string());
            std::ptr::null_mut()
        }
    }
}

/// Reason the last `lingxi_lmp_normalizer_open` returned NULL.
#[no_mangle]
pub extern "C" fn lingxi_normalizer_open_error() -> *mut c_char {
    static LAST_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    let message = LAST_ERROR
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
        .unwrap_or_else(|| "unknown normalizer error".to_string());
    error_envelope("open_failed", message)
}

/// Feeds one upstream frame and returns the LMP events it produced.
///
/// `event_json` may be either `{"data": "...", "event": "name"}` or a bare
/// JSON value that is already a provider frame. Envelope:
/// `{"ok":true,"events":[...]}`; a decode failure is `{"ok":false,...}`.
#[no_mangle]
pub extern "C" fn lingxi_lmp_normalizer_push(
    ptr: *mut LingxiNormalizer,
    event_json_utf8: *const c_char,
) -> *mut c_char {
    let Some(session) = resolve(ptr) else {
        return error_envelope("invalid_handle", "null or released normalizer handle");
    };
    let Some(raw) = c_str(event_json_utf8) else {
        return error_envelope("invalid_request", "null or invalid event JSON");
    };
    let value: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(e) => return error_envelope("invalid_request", e.to_string()),
    };
    let event = SseEvent {
        event: value.get("event").and_then(Value::as_str).map(ToString::to_string),
        data: match value.get("data") {
            Some(Value::String(text)) => text.clone(),
            Some(other) => other.to_string(),
            None => value.to_string(),
        },
        id: value.get("id").and_then(Value::as_str).map(ToString::to_string),
    };
    let mut guard = match session.normalizer.lock() {
        Ok(guard) => guard,
        Err(_) => return error_envelope("invalid_handle", "normalizer handle poisoned"),
    };
    let Some(normalizer) = guard.as_mut() else {
        return to_c_json(json!({"ok": true, "events": [], "done": true}));
    };
    let mut out = Vec::new();
    match normalizer.push(&event, &mut out) {
        Ok(()) => to_c_json(json!({"ok": true, "events": out, "done": normalizer.is_finished()})),
        Err(err) => error_envelope(err.code(), err.to_string()),
    }
}

/// Flushes trailing state (a final `message_end` when the provider omitted one).
#[no_mangle]
pub extern "C" fn lingxi_lmp_normalizer_finish(ptr: *mut LingxiNormalizer) -> *mut c_char {
    let Some(session) = resolve(ptr) else {
        return error_envelope("invalid_handle", "null or released normalizer handle");
    };
    let mut guard = match session.normalizer.lock() {
        Ok(guard) => guard,
        Err(_) => return error_envelope("invalid_handle", "normalizer handle poisoned"),
    };
    let mut out = Vec::new();
    if let Some(normalizer) = guard.as_mut() {
        normalizer.finish(&mut out);
    }
    session.finished.store(true, Ordering::SeqCst);
    to_c_json(json!({"ok": true, "events": out, "done": true, "protocol": session.protocol.as_str()}))
}

/// Releases a session. Idempotent and NULL-safe.
#[no_mangle]
pub extern "C" fn lingxi_lmp_normalizer_close(ptr: *mut LingxiNormalizer) {
    if ptr.is_null() {
        return;
    }
    let id = ptr as usize as u64;
    if let Ok(mut live) = sessions().lock() {
        live.remove(&id);
    }
}
