use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use serde_json::{json, Value};

use crate::lmp::{normalize_response, translate_request, LmpRequest};
use crate::registry::NativeProtocol;

fn parse_protocol(raw: &str) -> Option<NativeProtocol> {
    match raw {
        "openai_chat_completions" => Some(NativeProtocol::OpenaiChatCompletions),
        "openai_responses" => Some(NativeProtocol::OpenaiResponses),
        "anthropic_messages" => Some(NativeProtocol::AnthropicMessages),
        "google_gemini" => Some(NativeProtocol::GoogleGemini),
        _ => None,
    }
}

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

/// Translates an LMP JSON request into a provider-native JSON payload.
/// Caller must free the returned string with `lingxi_string_free`.
#[no_mangle]
pub extern "C" fn lingxi_lmp_translate_request(
    protocol_utf8: *const c_char,
    lmp_request_json_utf8: *const c_char,
) -> *mut c_char {
    let Some(proto_str) = c_str_to_string(protocol_utf8) else {
        return to_c_json(json!({"ok": false, "error": "null or invalid protocol"}));
    };
    let Some(proto) = parse_protocol(&proto_str) else {
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

/// Normalizes a provider-native JSON response into an LMP response JSON envelope.
/// Caller must free the returned string with `lingxi_string_free`.
#[no_mangle]
pub extern "C" fn lingxi_lmp_normalize_response(
    protocol_utf8: *const c_char,
    native_response_json_utf8: *const c_char,
) -> *mut c_char {
    let Some(proto_str) = c_str_to_string(protocol_utf8) else {
        return to_c_json(json!({"ok": false, "error": "null or invalid protocol"}));
    };
    let Some(proto) = parse_protocol(&proto_str) else {
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

/// Frees a string returned by `lingxi_lmp_*`. Safe to call with NULL.
#[no_mangle]
pub extern "C" fn lingxi_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            drop(CString::from_raw(ptr));
        }
    }
}
