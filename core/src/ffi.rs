use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use serde_json::{json, Value};

use crate::lmp::{normalize_response, normalize_stream_events, translate_request, LmpRequest};
use crate::registry::{NativeProtocol, ProviderRegistry};
use crate::sse::SseEvent;

fn c_str_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(ToString::to_string)
}

fn to_c_json(value: Value) -> *mut c_char {
    let text = value.to_string();
    match CString::new(text) {
        Ok(cs) => cs.into_raw(),
        Err(_) => CString::new(r#"{"ok":false,"error":"interior nul byte"}"#)
            .unwrap()
            .into_raw(),
    }
}

#[no_mangle]
pub extern "C" fn lingxi_lmp_translate_request(
    protocol_utf8: *const c_char,
    lmp_request_json_utf8: *const c_char,
) -> *mut c_char {
    let Some(proto_str) = c_str_to_string(protocol_utf8) else {
        return to_c_json(json!({"ok": false, "code": "invalid_request", "error": "null or invalid protocol"}));
    };
    let Some(proto) = NativeProtocol::parse_str(&proto_str) else {
        return to_c_json(
            json!({"ok": false, "code": "unsupported_protocol", "error": format!("unknown protocol: {proto_str}")}),
        );
    };
    let Some(req_str) = c_str_to_string(lmp_request_json_utf8) else {
        return to_c_json(json!({"ok": false, "code": "invalid_request", "error": "null or invalid request JSON"}));
    };
    let req: LmpRequest = match serde_json::from_str(&req_str) {
        Ok(r) => r,
        Err(e) => {
            return to_c_json(json!({"ok": false, "code": "invalid_request", "error": e.to_string()}))
        }
    };
    match translate_request(proto, &req) {
        Ok(payload) => to_c_json(json!({"ok": true, "payload": payload})),
        Err(e) => to_c_json(json!({"ok": false, "code": e.code(), "error": e.to_string()})),
    }
}

#[no_mangle]
pub extern "C" fn lingxi_lmp_normalize_response(
    protocol_utf8: *const c_char,
    native_response_json_utf8: *const c_char,
) -> *mut c_char {
    let Some(proto_str) = c_str_to_string(protocol_utf8) else {
        return to_c_json(json!({"ok": false, "code": "invalid_request", "error": "null or invalid protocol"}));
    };
    let Some(proto) = NativeProtocol::parse_str(&proto_str) else {
        return to_c_json(json!({"ok": false, "code": "unsupported_protocol", "error": format!("unknown protocol: {proto_str}")}));
    };
    let Some(resp_str) = c_str_to_string(native_response_json_utf8) else {
        return to_c_json(json!({"ok": false, "code": "invalid_request", "error": "null or invalid response JSON"}));
    };
    let raw: Value = match serde_json::from_str(&resp_str) {
        Ok(v) => v,
        Err(e) => return to_c_json(json!({"ok": false, "code": "invalid_request", "error": e.to_string()})),
    };
    match normalize_response(proto, &raw) {
        Ok(norm) => to_c_json(json!({"ok": true, "response": norm})),
        Err(e) => to_c_json(json!({"ok": false, "code": e.code(), "error": e.to_string()})),
    }
}

#[no_mangle]
pub extern "C" fn lingxi_lmp_normalize_stream_events(
    protocol_utf8: *const c_char,
    events_json_utf8: *const c_char,
) -> *mut c_char {
    let Some(proto_str) = c_str_to_string(protocol_utf8) else {
        return to_c_json(json!({"ok": false, "code": "invalid_request", "error": "null or invalid protocol"}));
    };
    let Some(proto) = NativeProtocol::parse_str(&proto_str) else {
        return to_c_json(json!({"ok": false, "code": "unsupported_protocol", "error": format!("unknown protocol: {proto_str}")}));
    };
    let Some(ev_str) = c_str_to_string(events_json_utf8) else {
        return to_c_json(json!({"ok": false, "code": "invalid_request", "error": "null or invalid events JSON"}));
    };
    let raw_events: Vec<Value> = match serde_json::from_str(&ev_str) {
        Ok(v) => v,
        Err(e) => return to_c_json(json!({"ok": false, "code": "invalid_request", "error": e.to_string()})),
    };
    let events: Vec<SseEvent> = raw_events
        .into_iter()
        .map(|v| SseEvent {
            event: v["event"].as_str().map(ToString::to_string),
            data: v["data"].as_str().unwrap_or("").to_string(),
            id: v["id"].as_str().map(ToString::to_string),
        })
        .collect();
    match normalize_stream_events(proto, &events) {
        Ok(norm) => to_c_json(json!({"ok": true, "events": norm})),
        Err(e) => to_c_json(json!({"ok": false, "code": e.code(), "error": e.to_string()})),
    }
}

#[no_mangle]
pub extern "C" fn lingxi_registry_resolve_endpoint(
    registry_json_utf8: *const c_char,
    provider_id_utf8: *const c_char,
    protocol_utf8: *const c_char,
) -> *mut c_char {
    let Some(reg_str) = c_str_to_string(registry_json_utf8) else {
        return to_c_json(json!({"ok": false, "code": "registry", "error": "null registry JSON"}));
    };
    let Some(pid) = c_str_to_string(provider_id_utf8) else {
        return to_c_json(json!({"ok": false, "code": "registry", "error": "null provider_id"}));
    };
    let Some(proto_str) = c_str_to_string(protocol_utf8) else {
        return to_c_json(json!({"ok": false, "code": "registry", "error": "null protocol"}));
    };
    let Some(proto) = NativeProtocol::parse_str(&proto_str) else {
        return to_c_json(json!({"ok": false, "code": "unsupported_protocol", "error": format!("unknown protocol: {proto_str}")}));
    };
    let reg = match ProviderRegistry::from_json(reg_str.as_bytes()) {
        Ok(r) => r,
        Err(e) => return to_c_json(json!({"ok": false, "code": e.code(), "error": e.to_string()})),
    };
    match reg.resolve_endpoint(&pid, proto, None, None, false) {
        Ok((entry, endpoint)) => to_c_json(json!({
            "ok": true,
            "endpoint": endpoint,
            "provider": entry.id,
            "authType": entry.auth.auth_type,
        })),
        Err(e) => to_c_json(json!({"ok": false, "code": e.code(), "error": e.to_string()})),
    }
}

#[no_mangle]
pub extern "C" fn lingxi_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            drop(CString::from_raw(ptr));
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmbedRequest {
    protocol: String,
    base_url: String,
    api_key: Option<String>,
    model: String,
    input: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RerankRequestWire {
    protocol: String,
    base_url: String,
    api_key: Option<String>,
    model: String,
    query: String,
    documents: Vec<String>,
    top_n: Option<usize>,
}

fn shared_runtime() -> Result<&'static tokio::runtime::Runtime, String> {
    static RUNTIME: std::sync::OnceLock<std::io::Result<tokio::runtime::Runtime>> =
        std::sync::OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
        })
        .as_ref()
        .map_err(|e| format!("could not start Rust HTTP runtime: {e}"))
}

/// Normalized embedding through the Rust core.
#[no_mangle]
pub extern "C" fn lingxi_unified_embed(request_json_utf8: *const c_char) -> *mut c_char {
    let Some(raw) = c_str_to_string(request_json_utf8) else {
        return to_c_json(json!({"ok": false, "code": "invalid_request", "error": "null embed request"}));
    };
    let Ok(request) = serde_json::from_str::<EmbedRequest>(&raw) else {
        return to_c_json(json!({"ok": false, "code": "invalid_request", "error": "invalid embed request JSON"}));
    };
    let Some(protocol) = NativeProtocol::parse_str(&request.protocol) else {
        return to_c_json(json!({"ok": false, "code": "unsupported_protocol", "error": "unknown protocol"}));
    };
    if tokio::runtime::Handle::try_current().is_ok() {
        return to_c_json(json!({"ok": false, "code": "invalid_context", "error": "blocking C ABI must run outside an async runtime"}));
    }
    let Ok(runtime) = shared_runtime() else {
        return to_c_json(json!({"ok": false, "code": "runtime", "error": "could not start Rust HTTP runtime"}));
    };
    let result = runtime.block_on(async {
        static UNIFIED: std::sync::OnceLock<crate::lmp::UnifiedClient> = std::sync::OnceLock::new();
        let client = UNIFIED.get_or_init(crate::lmp::UnifiedClient::new);
        let req = crate::lmp::EmbeddingRequest {
            model: request.model,
            input: request.input,
        };
        client
            .embed(protocol, &request.base_url, request.api_key, &req)
            .await
    });
    match result {
        Ok(response) => to_c_json(json!({"ok": true, "response": response})),
        Err(e) => to_c_json(json!({"ok": false, "code": e.code(), "error": e.to_string()})),
    }
}

/// Normalized rerank through the Rust core.
#[no_mangle]
pub extern "C" fn lingxi_unified_rerank(request_json_utf8: *const c_char) -> *mut c_char {
    let Some(raw) = c_str_to_string(request_json_utf8) else {
        return to_c_json(json!({"ok": false, "code": "invalid_request", "error": "null rerank request"}));
    };
    let Ok(request) = serde_json::from_str::<RerankRequestWire>(&raw) else {
        return to_c_json(json!({"ok": false, "code": "invalid_request", "error": "invalid rerank request JSON"}));
    };
    let Some(protocol) = NativeProtocol::parse_str(&request.protocol) else {
        return to_c_json(json!({"ok": false, "code": "unsupported_protocol", "error": "unknown protocol"}));
    };
    if tokio::runtime::Handle::try_current().is_ok() {
        return to_c_json(json!({"ok": false, "code": "invalid_context", "error": "blocking C ABI must run outside an async runtime"}));
    }
    let Ok(runtime) = shared_runtime() else {
        return to_c_json(json!({"ok": false, "code": "runtime", "error": "could not start Rust HTTP runtime"}));
    };
    let result = runtime.block_on(async {
        static UNIFIED: std::sync::OnceLock<crate::lmp::UnifiedClient> = std::sync::OnceLock::new();
        let client = UNIFIED.get_or_init(crate::lmp::UnifiedClient::new);
        let req = crate::lmp::RerankRequest {
            model: request.model,
            query: request.query,
            documents: request.documents,
            top_n: request.top_n,
        };
        client
            .rerank(protocol, &request.base_url, request.api_key, &req)
            .await
    });
    match result {
        Ok(response) => to_c_json(json!({"ok": true, "response": response})),
        Err(e) => to_c_json(json!({"ok": false, "code": e.code(), "error": e.to_string()})),
    }
}


#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrepareRequest {
    protocol: String,
    base_url: String,
    api_key: Option<String>,
    model: Option<String>,
    payload: Value,
    #[serde(default)]
    headers: std::collections::HashMap<String, String>,
    timeout_ms: Option<u64>,
    idle_timeout_ms: Option<u64>,
    #[serde(default)]
    allow_insecure_http: bool,
    #[serde(default)]
    stream: bool,
}

/// Resolves the final URL, auth headers, request body and wire framing for one
/// native request. Foreign SDKs use this instead of re-implementing endpoint
/// joining, protocol auth headers or AWS SigV4. No network I/O happens here, so
/// it is safe to call from any thread, including inside a host async loop.
#[no_mangle]
pub extern "C" fn lingxi_native_prepare(request_json_utf8: *const c_char) -> *mut c_char {
    let Some(raw) = c_str_to_string(request_json_utf8) else {
        return to_c_json(json!({"ok": false, "code": "invalid_request", "error": "null prepare request"}));
    };
    let Ok(request) = serde_json::from_str::<PrepareRequest>(&raw) else {
        return to_c_json(json!({"ok": false, "code": "invalid_request", "error": "invalid prepare request JSON"}));
    };
    let Some(protocol) = NativeProtocol::parse_str(&request.protocol) else {
        return to_c_json(json!({"ok": false, "code": "unsupported_protocol", "error": format!("unknown protocol: {}", request.protocol)}));
    };
    let mut config = crate::NativeRequestConfig::new(request.base_url, request.api_key);
    config.headers = request.headers;
    config.allow_insecure_http = request.allow_insecure_http;
    if let Some(timeout) = request.timeout_ms {
        config.timeout = std::time::Duration::from_millis(timeout);
    }
    if let Some(idle) = request.idle_timeout_ms {
        config.stream_idle_timeout = std::time::Duration::from_millis(idle);
    }
    match crate::native::prepare_target(
        protocol,
        &config,
        request.model.as_deref(),
        &request.payload,
        request.stream,
    ) {
        Ok(target) => to_c_json(json!({
            "ok": true,
            "url": target.url,
            "headers": target.headers,
            "body": target.body,
            "framing": target.framing.as_str(),
        })),
        Err(e) => {
            to_c_json(json!({"ok": false, "code": e.code(), "error": e.to_string()}))
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeRequest {
    protocol: String,
    base_url: String,
    api_key: Option<String>,
    model: Option<String>,
    payload: Value,
    #[serde(default)]
    headers: std::collections::HashMap<String, String>,
    timeout_ms: Option<u64>,
    #[serde(default)]
    unified: bool,
    #[serde(default)]
    stream: bool,
}


#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct UnifiedPrepareRequest {
    protocol: String,
    base_url: String,
    api_key: Option<String>,
    model: Option<String>,
    request: Value,
    #[serde(default)]
    headers: std::collections::HashMap<String, String>,
    timeout_ms: Option<u64>,
    idle_timeout_ms: Option<u64>,
    #[serde(default)]
    allow_insecure_http: bool,
    #[serde(default)]
    stream: bool,
}

/// One-call LMP -> wire translation for foreign SDKs: turns an LMP request into
/// the final URL, auth headers, provider-native body and framing. No network
/// I/O, so calling it from a host thread is always safe.
#[no_mangle]
pub extern "C" fn lingxi_unified_prepare(request_json_utf8: *const c_char) -> *mut c_char {
    let Some(raw) = c_str_to_string(request_json_utf8) else {
        return to_c_json(json!({"ok": false, "code": "invalid_request", "error": "null unified prepare request"}));
    };
    let Ok(request) = serde_json::from_str::<UnifiedPrepareRequest>(&raw) else {
        return to_c_json(json!({"ok": false, "code": "invalid_request", "error": "invalid unified prepare request JSON"}));
    };
    let Some(protocol) = NativeProtocol::parse_str(&request.protocol) else {
        return to_c_json(json!({"ok": false, "code": "unsupported_protocol", "error": format!("unknown protocol: {}", request.protocol)}));
    };
    let Ok(mut lmp) = serde_json::from_value::<LmpRequest>(request.request) else {
        return to_c_json(json!({"ok": false, "code": "invalid_request", "error": "invalid LMP request payload"}));
    };
    if let Some(timeout) = request.timeout_ms {
        lmp.timeout_ms = Some(timeout);
    }
    let payload = match crate::lmp::translate_request(protocol, &lmp) {
        Ok(payload) => payload,
        Err(e) => return to_c_json(json!({"ok": false, "code": e.code(), "error": e.to_string()})),
    };
    let model = request.model.clone().or_else(|| Some(lmp.model.clone()));
    let mut config = crate::NativeRequestConfig::new(request.base_url, request.api_key);
    config.headers = request.headers;
    config.allow_insecure_http = request.allow_insecure_http;
    if let Some(timeout) = request.timeout_ms {
        config.timeout = std::time::Duration::from_millis(timeout);
    }
    if let Some(idle) = request.idle_timeout_ms {
        config.stream_idle_timeout = std::time::Duration::from_millis(idle);
    }
    match crate::native::prepare_target(
        protocol,
        &config,
        model.as_deref(),
        &payload,
        request.stream,
    ) {
        Ok(target) => to_c_json(json!({
            "ok": true,
            "protocol": protocol.as_str(),
            "url": target.url,
            "headers": target.headers,
            "body": target.body,
            "framing": target.framing.as_str(),
        })),
        Err(e) => {
            to_c_json(json!({"ok": false, "code": e.code(), "error": e.to_string()}))
        }
    }
}

/// Makes a blocking network call. Call from a native thread outside an async runtime.
#[no_mangle]
pub extern "C" fn lingxi_runtime_request(request_json_utf8: *const c_char) -> *mut c_char {
    let err = |code: &str, message: &str| to_c_json(json!({"ok": false, "code": code, "error": message}));
    let Some(raw) = c_str_to_string(request_json_utf8) else {
        return err("invalid_request", "null or invalid runtime request");
    };
    let Ok(request) = serde_json::from_str::<RuntimeRequest>(&raw) else {
        return err("invalid_request", "invalid runtime request JSON");
    };
    let Some(protocol) = NativeProtocol::parse_str(&request.protocol) else {
        return err("unsupported_protocol", "unknown protocol");
    };
    if tokio::runtime::Handle::try_current().is_ok() {
        return err("invalid_context", "blocking C ABI must run outside an async runtime");
    }

    static RUNTIME: std::sync::OnceLock<std::io::Result<tokio::runtime::Runtime>> = std::sync::OnceLock::new();
    static NATIVE: std::sync::OnceLock<crate::NativeClient> = std::sync::OnceLock::new();
    static UNIFIED: std::sync::OnceLock<crate::lmp::UnifiedClient> = std::sync::OnceLock::new();
    let Ok(runtime) = RUNTIME.get_or_init(|| tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2).enable_all().build()) else {
        return err("runtime", "could not start Rust HTTP runtime");
    };

    let response = runtime.block_on(async {
        if request.unified {
            if !request.headers.is_empty() {
                return json!({"ok": false, "code": "invalid_request", "error": "unified request does not accept custom HTTP headers"});
            }
            let Ok(mut payload) = serde_json::from_value::<LmpRequest>(request.payload) else {
                return json!({"ok": false, "code": "invalid_request", "error": "invalid unified request payload"});
            };
            payload.timeout_ms = request.timeout_ms.or(payload.timeout_ms);
            let client = UNIFIED.get_or_init(crate::lmp::UnifiedClient::new);
            if request.stream {
                return match client.stream(protocol, &request.base_url, request.api_key, &payload).await {
                    Ok(events) => json!({"ok": true, "events": events}),
                    Err(e) => json!({"ok": false, "code": e.code(), "error": e.to_string()}),
                };
            }
            return match client.complete(protocol, &request.base_url, request.api_key, &payload).await {
                Ok(response) => json!({"ok": true, "response": response}),
                Err(e) => json!({"ok": false, "code": e.code(), "error": e.to_string()}),
            };
        }
        let mut config = crate::NativeRequestConfig::new(request.base_url, request.api_key);
        config.headers = request.headers;
        if let Some(timeout) = request.timeout_ms {
            config.timeout = std::time::Duration::from_millis(timeout);
        }
        let client = NATIVE.get_or_init(crate::NativeClient::new);
        if request.stream {
            return match client.request_stream(protocol, &config, request.model.as_deref(), &request.payload).await {
                Ok(events) => json!({"ok": true, "events": events.into_iter().map(|event| json!({
                    "event": event.event, "data": event.data, "id": event.id
                })).collect::<Vec<_>>() }),
                Err(e) => json!({"ok": false, "code": e.code(), "error": e.to_string()}),
            };
        }
        match client.request_json(protocol, &config, request.model.as_deref(), &request.payload).await {
            Ok(response) => json!({"ok": true, "status": response.status, "headers": response.headers, "body": response.body, "rawBody": String::from_utf8_lossy(&response.raw_bytes)}),
            Err(crate::NativeError::HttpStatus { status, headers, body }) => json!({
                "ok": false, "code": "http_status", "status": status, "headers": headers, "body": String::from_utf8_lossy(&body)
            }),
            Err(e) => json!({"ok": false, "code": e.code(), "error": e.to_string()}),
        }
    });
    to_c_json(response)
}
