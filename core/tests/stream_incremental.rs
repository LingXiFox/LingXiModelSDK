//! Incremental streaming: latency, cancellation, early exit, abnormal
//! disconnects, concurrency, and the C ABI stream-handle lifecycle.

use std::ffi::{CStr, CString};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::raw::c_char;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use lingxi_core::lmp::{LmpRequest, LmpStreamEvent};
use lingxi_core::{NativeClient, NativeProtocol, NativeRequestConfig};
use serde_json::{json, Value};

extern "C" {
    fn lingxi_stream_open(request_json: *const c_char) -> *mut std::ffi::c_void;
    fn lingxi_stream_open_error() -> *mut c_char;
    fn lingxi_stream_next(handle: *mut std::ffi::c_void, timeout_ms: u64) -> *mut c_char;
    fn lingxi_stream_cancel(handle: *mut std::ffi::c_void);
    fn lingxi_stream_close(handle: *mut std::ffi::c_void);
    fn lingxi_stream_last_error(handle: *mut std::ffi::c_void) -> *mut c_char;
    fn lingxi_string_free(ptr: *mut c_char);
}

fn take(ptr: *mut c_char) -> Value {
    assert!(!ptr.is_null(), "ABI returned NULL instead of an envelope");
    let text = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
    unsafe { lingxi_string_free(ptr) };
    serde_json::from_str(&text).expect("envelope is JSON")
}

fn c_string(value: Value) -> CString {
    CString::new(value.to_string()).unwrap()
}

/// One scripted SSE frame the test server should emit.
struct Frame {
    payload: String,
    /// Delay applied *before* writing this frame.
    delay: Duration,
}

fn frame(payload: &str, delay_ms: u64) -> Frame {
    Frame {
        payload: payload.to_string(),
        delay: Duration::from_millis(delay_ms),
    }
}

/// Serves a chunked `text/event-stream` response, writing one frame at a time.
///
/// Returns the base URL plus a flag that flips true once the script finished,
/// so tests can prove the client saw events *before* the server was done.
fn serve_sse(frames: Vec<Frame>) -> (String, Arc<AtomicBool>, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let finished = Arc::new(AtomicBool::new(false));
    let requests = Arc::new(AtomicUsize::new(0));
    let done = Arc::clone(&finished);
    let count = Arc::clone(&requests);
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut socket) = incoming else { break };
            count.fetch_add(1, Ordering::SeqCst);
            let mut scratch = [0u8; 8192];
            let _ = socket.read(&mut scratch);
            let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: keep-alive\r\n\r\n";
            if socket.write_all(head.as_bytes()).is_err() {
                break;
            }
            let mut broken = false;
            for frame in &frames {
                thread::sleep(frame.delay);
                let chunk = format!("{:x}\r\n{}\r\n", frame.payload.len(), frame.payload);
                if socket.write_all(chunk.as_bytes()).and_then(|_| socket.flush()).is_err() {
                    broken = true;
                    break;
                }
            }
            if !broken {
                let _ = socket.write_all(b"0\r\n\r\n");
            }
            done.store(true, Ordering::SeqCst);
        }
    });
    (format!("http://{addr}"), finished, requests)
}

/// Serves a chunked response that is abruptly closed in the middle of a frame.
fn serve_truncated_sse(first: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let first = first.to_string();
    thread::spawn(move || {
        let Ok((mut socket, _)) = listener.accept() else { return };
        let mut scratch = [0u8; 8192];
        let _ = socket.read(&mut scratch);
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n";
        let _ = socket.write_all(head.as_bytes());
        let chunk = format!("{:x}\r\n{}\r\n", first.len(), first);
        let _ = socket.write_all(chunk.as_bytes());
        let _ = socket.flush();
        // Drop without the terminating zero chunk: an abnormal end of stream.
    });
    format!("http://{addr}")
}

/// Serves headers immediately and then stays silent forever.
fn serve_silent() -> (String, Arc<AtomicBool>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let open = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&open);
    thread::spawn(move || {
        let Ok((mut socket, _)) = listener.accept() else { return };
        let mut scratch = [0u8; 8192];
        let _ = socket.read(&mut scratch);
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n";
        let _ = socket.write_all(head.as_bytes());
        let _ = socket.flush();
        // Park until the test drops the connection.
        let mut sink = [0u8; 1024];
        let _ = socket.read(&mut sink);
        flag.store(false, Ordering::SeqCst);
    });
    (format!("http://{addr}"), open)
}

fn cfg(base_url: &str) -> NativeRequestConfig {
    let mut config = NativeRequestConfig::new(base_url.to_string(), Some("test-key".into()));
    config.timeout = Duration::from_secs(5);
    config.stream_idle_timeout = Duration::from_secs(5);
    config
}

fn chat_payload() -> Value {
    json!({"model": "test-model", "messages": [{"role": "user", "content": "hi"}], "stream": true})
}

fn text_frame(text: &str) -> String {
    format!("data: {}\n\n", json!({"choices": [{"delta": {"content": text}}]}))
}

#[tokio::test]
async fn streams_events_before_upstream_finishes() {
    let (url, finished, _) = serve_sse(vec![
        frame(&text_frame("first"), 30),
        frame(&text_frame("second"), 30),
        frame("data: [DONE]\n\n", 30),
    ]);
    let client = NativeClient::new();
    let started = Instant::now();
    let mut handle = client
        .open_stream_events(NativeProtocol::OpenaiChatCompletions, &cfg(&url), None, &chat_payload())
        .await
        .unwrap();
    assert_eq!(handle.status(), 200);

    let first = handle.next_event().await.unwrap().unwrap();
    let first_at = started.elapsed();
    assert!(first.data.contains("first"));
    // The whole script takes ~90ms; the first event must arrive well before
    // the upstream response is complete.
    assert!(
        first_at < Duration::from_millis(60),
        "first event took {first_at:?}, so the body was buffered"
    );
    assert!(!finished.load(Ordering::SeqCst), "server already finished; not incremental");

    let mut texts = String::new();
    while let Some(event) = handle.next_event().await.unwrap() {
        texts.push_str(&event.data);
    }
    assert!(texts.contains("second"));
    assert!(finished.load(Ordering::SeqCst));
    assert!(handle.bytes_read() > 0);
}

#[tokio::test]
async fn early_exit_releases_the_connection() {
    let frames: Vec<Frame> = (0..40)
        .map(|i| frame(&text_frame(&format!("chunk-{i}")), 10))
        .collect();
    let (url, finished, _) = serve_sse(frames);
    let client = NativeClient::new();
    {
        let mut handle = client
            .open_stream_events(NativeProtocol::OpenaiChatCompletions, &cfg(&url), None, &chat_payload())
            .await
            .unwrap();
        let first = handle.next_event().await.unwrap().unwrap();
        assert!(first.data.contains("chunk-0"));
        // Dropping here must close the socket, not drain the remaining frames.
    }
    // The server never completed its 40-frame script.
    assert!(!finished.load(Ordering::SeqCst));
}

#[tokio::test]
async fn cancel_interrupts_a_blocked_read() {
    let (url, _) = serve_silent();
    let client = NativeClient::new();
    let mut handle = client
        .open_stream_events(NativeProtocol::OpenaiChatCompletions, &cfg(&url), None, &chat_payload())
        .await
        .unwrap();
    handle.cancel();
    let started = Instant::now();
    let err = handle.next_event().await.unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(1), "cancel did not interrupt the read");
    assert!(matches!(err, lingxi_core::NativeError::Cancelled(_)), "unexpected error: {err}");
    assert!(handle.is_cancelled());
}

#[tokio::test]
async fn cancel_from_another_task_while_reading() {
    let (url, _) = serve_silent();
    let client = NativeClient::new();
    let mut handle = client
        .open_stream_events(NativeProtocol::OpenaiChatCompletions, &cfg(&url), None, &chat_payload())
        .await
        .unwrap();
    let token = handle.cancellation();
    let notify = handle.cancellation_notify();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        token.store(true, Ordering::SeqCst);
        notify.notify_waiters();
    });
    let started = Instant::now();
    let err = handle.next_event().await.unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(2), "blocked read ignored the token");
    assert!(matches!(err, lingxi_core::NativeError::Cancelled(_)));
}

#[tokio::test]
async fn idle_timeout_fires_when_upstream_goes_quiet() {
    let (url, _) = serve_silent();
    let mut config = cfg(&url);
    config.stream_idle_timeout = Duration::from_millis(120);
    let client = NativeClient::new();
    let mut handle = client
        .open_stream_events(NativeProtocol::OpenaiChatCompletions, &config, None, &chat_payload())
        .await
        .unwrap();
    let err = handle.next_event().await.unwrap_err();
    assert!(matches!(err, lingxi_core::NativeError::Timeout(_)), "unexpected: {err}");
}

#[tokio::test]
async fn truncated_stream_reports_an_error_not_a_clean_end() {
    let url = serve_truncated_sse(&text_frame("partial"));
    let client = NativeClient::new();
    let mut handle = client
        .open_stream_events(NativeProtocol::OpenaiChatCompletions, &cfg(&url), None, &chat_payload())
        .await
        .unwrap();
    let mut saw_text = false;
    let mut error = None;
    loop {
        match handle.next_event().await {
            Ok(Some(event)) => saw_text |= event.data.contains("partial"),
            Ok(None) => break,
            Err(err) => {
                error = Some(err.to_string());
                break;
            }
        }
    }
    assert!(saw_text, "the delivered fragment should still be visible");
    assert!(
        error.is_some(),
        "a truncated chunked body must surface an error instead of a clean end"
    );
}

#[tokio::test]
async fn concurrent_streams_stay_independent() {
    let (url_a, _, _) = serve_sse(vec![
        frame(&text_frame("alpha-1"), 20),
        frame(&text_frame("alpha-2"), 20),
        frame("data: [DONE]\n\n", 5),
    ]);
    let (url_b, _, _) = serve_sse(vec![
        frame(&text_frame("beta-1"), 20),
        frame(&text_frame("beta-2"), 20),
        frame("data: [DONE]\n\n", 5),
    ]);
    let client = Arc::new(NativeClient::new());
    let mut tasks = Vec::new();
    for url in [url_a, url_b] {
        let client = Arc::clone(&client);
        tasks.push(tokio::spawn(async move {
            let mut handle = client
                .open_stream_events(
                    NativeProtocol::OpenaiChatCompletions,
                    &cfg(&url),
                    None,
                    &chat_payload(),
                )
                .await
                .unwrap();
            let mut out = String::new();
            while let Some(event) = handle.next_event().await.unwrap() {
                out.push_str(&event.data);
            }
            out
        }));
    }
    let a = tasks.remove(0).await.unwrap();
    let b = tasks.remove(0).await.unwrap();
    assert!(a.contains("alpha-1") && a.contains("alpha-2"));
    assert!(b.contains("beta-1") && b.contains("beta-2"));
    assert!(!a.contains("beta-1"));
    assert!(!b.contains("alpha-1"));
}

#[test]
fn ffi_stream_handle_lifecycle_is_memory_safe() {
    let (url, _, requests) = serve_sse(vec![
        frame(&text_frame("ffi-1"), 5),
        frame(&text_frame("ffi-2"), 5),
        frame("data: [DONE]\n\n", 5),
    ]);
    let request = c_string(json!({
        "protocol": "openai_chat_completions",
        "baseUrl": url,
        "apiKey": "test-key",
        "payload": chat_payload(),
        "idleTimeoutMs": 5000,
    }));
    let handle = unsafe { lingxi_stream_open(request.as_ptr()) };
    assert!(!handle.is_null(), "open failed: {:?}", take(unsafe { lingxi_stream_open_error() }));

    let first = take(unsafe { lingxi_stream_next(handle, 5000) });
    assert_eq!(first["ok"], json!(true));
    assert!(first["event"]["data"].as_str().unwrap().contains("ffi-1"));

    let mut done = false;
    let mut text = String::new();
    for _ in 0..10 {
        let envelope = take(unsafe { lingxi_stream_next(handle, 5000) });
        if envelope["done"] == json!(true) {
            done = true;
            break;
        }
        text.push_str(envelope["event"]["data"].as_str().unwrap());
    }
    assert!(done, "stream never reported completion");
    assert!(text.contains("ffi-2"));
    assert_eq!(requests.load(Ordering::SeqCst), 1, "should reuse one connection");

    // Close is idempotent and NULL-safe; later calls fail without crashing.
    unsafe {
        lingxi_stream_close(handle);
        lingxi_stream_close(handle);
        lingxi_stream_close(std::ptr::null_mut());
        lingxi_stream_cancel(std::ptr::null_mut());
    }
    let after = take(unsafe { lingxi_stream_next(handle, 10) });
    assert_eq!(after["ok"], json!(false));
    assert!(after["error"].as_str().unwrap().contains("released"));
    let last = take(unsafe { lingxi_stream_last_error(handle) });
    assert_eq!(last["ok"], json!(false));
}

#[test]
fn ffi_cancel_while_next_blocks() {
    let (url, _) = serve_silent();
    let request = c_string(json!({
        "protocol": "openai_chat_completions",
        "baseUrl": url,
        "apiKey": "test-key",
        "payload": chat_payload(),
        "idleTimeoutMs": 30000,
    }));
    let handle = unsafe { lingxi_stream_open(request.as_ptr()) };
    assert!(!handle.is_null());
    let token = handle as usize;
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(80));
        unsafe { lingxi_stream_cancel(token as *mut std::ffi::c_void) };
    });
    let started = Instant::now();
    let envelope = take(unsafe { lingxi_stream_next(handle, 0) });
    assert!(started.elapsed() < Duration::from_secs(2), "cancel did not unblock next");
    assert_eq!(envelope["ok"], json!(false));
    assert_eq!(envelope["code"], json!("cancelled"));
    let last = take(unsafe { lingxi_stream_last_error(handle) });
    assert_eq!(last["unified"], json!(false));
    unsafe { lingxi_stream_close(handle) };
}

#[test]
fn ffi_unified_stream_emits_lmp_events() {
    let (url, _, _) = serve_sse(vec![
        frame(
            &format!("data: {}\n\n", json!({"id": "chatcmpl-1", "choices": [{"delta": {"content": "Hel"}}]})),
            5,
        ),
        frame(
            &format!("data: {}\n\n", json!({"choices": [{"delta": {"content": "lo"}}]})),
            5,
        ),
        frame(
            &format!(
                "data: {}\n\n",
                json!({"choices": [{"delta": {}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5}})
            ),
            5,
        ),
        frame("data: [DONE]\n\n", 5),
    ]);
    let lmp = json!({
        "model": "test-model",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
        "max_tokens": 64,
    });
    let request = c_string(json!({
        "protocol": "openai_chat_completions",
        "baseUrl": url,
        "apiKey": "test-key",
        "payload": lmp,
        "unified": true,
        "idleTimeoutMs": 5000,
    }));
    let handle = unsafe { lingxi_stream_open(request.as_ptr()) };
    assert!(!handle.is_null(), "open failed: {:?}", take(unsafe { lingxi_stream_open_error() }));

    let mut text = String::new();
    let mut finish = None;
    let mut usage = None;
    loop {
        let envelope = take(unsafe { lingxi_stream_next(handle, 5000) });
        assert_eq!(envelope["ok"], json!(true), "stream failed: {envelope}");
        if envelope["done"] == json!(true) {
            break;
        }
        let event = &envelope["event"];
        match event["type"].as_str().unwrap() {
            "text_delta" => text.push_str(event["delta"].as_str().unwrap()),
            "message_end" => finish = event["finishReason"].as_str().map(ToString::to_string),
            "usage" => usage = Some(event["usage"].clone()),
            _ => {}
        }
    }
    assert_eq!(text, "Hello");
    assert_eq!(finish.as_deref(), Some("stop"));
    let usage = usage.expect("usage event missing");
    assert_eq!(usage["inputTokens"], json!(3));
    assert_eq!(usage["totalTokens"], json!(5));
    unsafe { lingxi_stream_close(handle) };
}

#[test]
fn ffi_open_reports_typed_failures() {
    let bad_protocol = c_string(json!({
        "protocol": "not_a_protocol",
        "baseUrl": "http://127.0.0.1:9",
        "payload": {},
    }));
    let handle = unsafe { lingxi_stream_open(bad_protocol.as_ptr()) };
    assert!(handle.is_null());
    let envelope = take(unsafe { lingxi_stream_open_error() });
    assert!(envelope["error"].as_str().unwrap().contains("unknown protocol"));

    let null = unsafe { lingxi_stream_open(std::ptr::null()) };
    assert!(null.is_null());
    let envelope = take(unsafe { lingxi_stream_open_error() });
    assert_eq!(envelope["ok"], json!(false));
}

#[tokio::test]
async fn unified_stream_surfaces_decode_errors_as_events() {
    // A provider that sends a chunk of invalid JSON mid-stream: the LMP handle
    // must emit a terminal `error` event instead of ending cleanly.
    let (url, _, _) = serve_sse(vec![
        frame(&text_frame("ok"), 5),
        frame("data: {not json}\n\n", 5),
    ]);
    let req = LmpRequest {
        model: "test-model".into(),
        messages: vec![lingxi_core::lmp::LmpMessage {
            role: lingxi_core::lmp::Role::User,
            content: vec![lingxi_core::lmp::ContentBlock::Text { text: "hi".into(), signature: None }],
        }],
        max_tokens: Some(16),
        temperature: None,
        tools: Vec::new(),
        tool_choice: None,
        thinking: None,
        response_format: None,
        timeout_ms: None,
        provider_extensions: Default::default(),
    };
    let client = lingxi_core::lmp::UnifiedClient::new();
    let mut handle = client
        .open_stream(NativeProtocol::OpenaiChatCompletions, &url, Some("k".into()), &req)
        .await
        .unwrap();
    let mut saw_error = None;
    while let Some(event) = handle.next_event().await.unwrap() {
        if let LmpStreamEvent::Error { code, message } = event {
            saw_error = Some((code, message));
        }
    }
    let (code, message) = saw_error.expect("decode failure must surface as an error event");
    assert_eq!(code, "decode");
    assert!(!message.is_empty());
    assert!(handle.failure().is_some());
    assert!(handle.is_finished());
}
