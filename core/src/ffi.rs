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
        return to_c_json(json!({"ok": false, "error": "null or invalid protocol"}));
    };
    let Some(proto) = NativeProtocol::parse_str(&proto_str) else {
        return to_c_json(json!({"ok": false, "error": format!("unknown protocol: {proto_str}")}));
    };
    let Some(req_str) = c_str_to_string(lmp_request_json_utf8) else {
        return to_c_json(json!({"ok": false, "error": "null or invalid request JSON"}));
    };
    let req: LmpRequest = match serde_json::from_str(&req_str) {
        Ok(r) => r,
        Err(e) => return to_c_json(json!({"ok": false, "error": e.to_string()})),
    };
    match translate_request(proto, &req) {
        Ok(payload) => to_c_json(json!({"ok": true, "payload": payload})),
        Err(e) => to_c_json(json!({"ok": false, "error": e.to_string()})),
    }
}

#[no_mangle]
pub extern "C" fn lingxi_lmp_normalize_response(
    protocol_utf8: *const c_char,
    native_response_json_utf8: *const c_char,
) -> *mut c_char {
    let Some(proto_str) = c_str_to_string(protocol_utf8) else {
        return to_c_json(json!({"ok": false, "error": "null or invalid protocol"}));
    };
    let Some(proto) = NativeProtocol::parse_str(&proto_str) else {
        return to_c_json(json!({"ok": false, "error": format!("unknown protocol: {proto_str}")}));
    };
    let Some(resp_str) = c_str_to_string(native_response_json_utf8) else {
        return to_c_json(json!({"ok": false, "error": "null or invalid response JSON"}));
    };
    let raw: Value = match serde_json::from_str(&resp_str) {
        Ok(v) => v,
        Err(e) => return to_c_json(json!({"ok": false, "error": e.to_string()})),
    };
    match normalize_response(proto, &raw) {
        Ok(norm) => to_c_json(json!({"ok": true, "response": norm})),
        Err(e) => to_c_json(json!({"ok": false, "error": e.to_string()})),
    }
}

#[no_mangle]
pub extern "C" fn lingxi_lmp_normalize_stream_events(
    protocol_utf8: *const c_char,
    events_json_utf8: *const c_char,
) -> *mut c_char {
    let Some(proto_str) = c_str_to_string(protocol_utf8) else {
        return to_c_json(json!({"ok": false, "error": "null or invalid protocol"}));
    };
    let Some(proto) = NativeProtocol::parse_str(&proto_str) else {
        return to_c_json(json!({"ok": false, "error": format!("unknown protocol: {proto_str}")}));
    };
    let Some(ev_str) = c_str_to_string(events_json_utf8) else {
        return to_c_json(json!({"ok": false, "error": "null or invalid events JSON"}));
    };
    let raw_events: Vec<Value> = match serde_json::from_str(&ev_str) {
        Ok(v) => v,
        Err(e) => return to_c_json(json!({"ok": false, "error": e.to_string()})),
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
        Err(e) => to_c_json(json!({"ok": false, "error": e.to_string()})),
    }
}

#[no_mangle]
pub extern "C" fn lingxi_registry_resolve_endpoint(
    registry_json_utf8: *const c_char,
    provider_id_utf8: *const c_char,
    protocol_utf8: *const c_char,
) -> *mut c_char {
    let Some(reg_str) = c_str_to_string(registry_json_utf8) else {
        return to_c_json(json!({"ok": false, "error": "null registry JSON"}));
    };
    let Some(pid) = c_str_to_string(provider_id_utf8) else {
        return to_c_json(json!({"ok": false, "error": "null provider_id"}));
    };
    let Some(proto_str) = c_str_to_string(protocol_utf8) else {
        return to_c_json(json!({"ok": false, "error": "null protocol"}));
    };
    let Some(proto) = NativeProtocol::parse_str(&proto_str) else {
        return to_c_json(json!({"ok": false, "error": format!("unknown protocol: {proto_str}")}));
    };
    let reg = match ProviderRegistry::from_json(reg_str.as_bytes()) {
        Ok(r) => r,
        Err(e) => return to_c_json(json!({"ok": false, "error": e.to_string()})),
    };
    match reg.resolve_endpoint(&pid, proto, None, None, false) {
        Ok((entry, endpoint)) => to_c_json(json!({
            "ok": true,
            "endpoint": endpoint,
            "provider": entry.id,
            "authType": entry.auth.auth_type,
        })),
        Err(e) => to_c_json(json!({"ok": false, "error": e.to_string()})),
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
