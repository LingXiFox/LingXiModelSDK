//! Covers `lingxi_native_prepare`: the single place where foreign SDKs obtain a
//! resolved URL, auth headers, body and wire framing.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use serde_json::{json, Value};

fn prepare(request: &Value) -> Value {
    let text = CString::new(request.to_string()).expect("no interior nul");
    let envelope = lingxi_core::ffi::lingxi_native_prepare(text.as_ptr());
    assert!(!envelope.is_null(), "prepare must always return an envelope");
    let body = unsafe { CStr::from_ptr(envelope) }.to_string_lossy().to_string();
    lingxi_core::ffi::lingxi_string_free(envelope);
    serde_json::from_str(&body).expect("envelope is JSON")
}

#[test]
fn openai_joins_chat_completions_and_adds_bearer() {
    let out = prepare(&json!({
        "protocol": "openai_chat_completions",
        "baseUrl": "https://api.example.com/v1",
        "apiKey": "sk-test",
        "payload": {"model": "gpt-4o", "messages": []}
    }));
    assert_eq!(out["ok"], json!(true));
    assert_eq!(out["url"], json!("https://api.example.com/v1/chat/completions"));
    assert_eq!(out["headers"]["Authorization"], json!("Bearer sk-test"));
    assert_eq!(out["framing"], json!("sse"));
    assert_eq!(out["body"]["model"], json!("gpt-4o"));
}

#[test]
fn anthropic_adds_version_header_and_messages_path() {
    let out = prepare(&json!({
        "protocol": "anthropic_messages",
        "baseUrl": "https://api.anthropic.com",
        "apiKey": "sk-ant",
        "payload": {"model": "claude-sonnet-4", "messages": []}
    }));
    assert_eq!(out["url"], json!("https://api.anthropic.com/v1/messages"));
    assert_eq!(out["headers"]["x-api-key"], json!("sk-ant"));
    assert_eq!(out["headers"]["anthropic-version"], json!("2023-06-01"));
}

#[test]
fn bedrock_signs_sigv4_without_leaking_the_secret() {
    let out = prepare(&json!({
        "protocol": "aws_bedrock",
        "baseUrl": "https://bedrock-runtime.us-west-2.amazonaws.com",
        "apiKey": "AKIDEXAMPLE:SECRETKEY",
        "model": "anthropic.claude-3-sonnet",
        "payload": {"messages": []}
    }));
    assert_eq!(out["ok"], json!(true));
    assert_eq!(
        out["url"],
        json!("https://bedrock-runtime.us-west-2.amazonaws.com/model/anthropic.claude-3-sonnet/converse")
    );
    assert_eq!(out["framing"], json!("amazon_event_stream"));
    let auth = out["headers"]["Authorization"].as_str().expect("sigv4 header");
    assert!(auth.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"), "{auth}");
    assert!(!auth.contains("SECRETKEY"), "secret must never appear in headers");
    assert!(out["headers"]["X-Amz-Date"].is_string());
}

#[test]
fn gemini_and_vertex_stream_paths_use_alt_sse() {
    let gemini = prepare(&json!({
        "protocol": "google_gemini",
        "baseUrl": "https://generativelanguage.googleapis.com",
        "apiKey": "g-key",
        "model": "gemini-2.0-flash",
        "payload": {},
        "stream": true
    }));
    assert_eq!(
        gemini["url"],
        json!("https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:streamGenerateContent?alt=sse")
    );
    assert_eq!(gemini["headers"]["x-goog-api-key"], json!("g-key"));

    let vertex = prepare(&json!({
        "protocol": "google_vertex_ai",
        "baseUrl": "https://us-central1-aiplatform.googleapis.com/v1/projects/p/locations/l",
        "apiKey": "v-key",
        "model": "gemini-2.0-flash",
        "payload": {}
    }));
    assert_eq!(
        vertex["url"],
        json!("https://us-central1-aiplatform.googleapis.com/v1/projects/p/locations/l/v1/publishers/google/models/gemini-2.0-flash:generateContent")
    );
}

#[test]
fn ollama_switches_stream_flag_and_uses_ndjson() {
    let plain = prepare(&json!({
        "protocol": "ollama",
        "baseUrl": "http://127.0.0.1:11434",
        "payload": {"model": "llama3"}
    }));
    assert_eq!(plain["body"]["stream"], json!(false));
    assert_eq!(plain["framing"], json!("ndjson"));

    let streamed = prepare(&json!({
        "protocol": "ollama",
        "baseUrl": "http://127.0.0.1:11434",
        "payload": {"model": "llama3"},
        "stream": true
    }));
    assert_eq!(streamed["body"]["stream"], json!(true));
}

#[test]
fn azure_honours_api_version_header_override() {
    let out = prepare(&json!({
        "protocol": "azure_openai",
        "baseUrl": "https://res.openai.azure.com",
        "apiKey": "az-key",
        "model": "gpt-4o",
        "payload": {"model": "gpt-4o"},
        "headers": {"x-azure-api-version": "2025-01-01-preview"}
    }));
    assert_eq!(
        out["url"],
        json!("https://res.openai.azure.com/openai/deployments/gpt-4o/chat/completions?api-version=2025-01-01-preview")
    );
    assert_eq!(out["headers"]["api-key"], json!("az-key"));
}

#[test]
fn invalid_inputs_report_typed_codes() {
    let unknown = prepare(&json!({
        "protocol": "not_a_protocol",
        "baseUrl": "https://example.com",
        "payload": {}
    }));
    assert_eq!(unknown["ok"], json!(false));
    assert_eq!(unknown["code"], json!("unsupported_protocol"));

    let insecure = prepare(&json!({
        "protocol": "openai_chat_completions",
        "baseUrl": "http://example.com/v1",
        "apiKey": "k",
        "payload": {}
    }));
    assert_eq!(insecure["ok"], json!(false));
    assert_eq!(insecure["code"], json!("invalid_endpoint"));

    let missing_key = prepare(&json!({
        "protocol": "openai_chat_completions",
        "baseUrl": "https://example.com/v1",
        "payload": {}
    }));
    assert_eq!(missing_key["ok"], json!(false));
    assert_eq!(missing_key["code"], json!("auth_missing"));

    let null_envelope = lingxi_core::ffi::lingxi_native_prepare(std::ptr::null::<c_char>());
    let text = unsafe { CStr::from_ptr(null_envelope) }.to_string_lossy().to_string();
    lingxi_core::ffi::lingxi_string_free(null_envelope);
    let parsed: Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!(parsed["code"], json!("invalid_request"));
}

#[test]
fn custom_headers_are_preserved_and_overridable() {
    let out = prepare(&json!({
        "protocol": "anthropic_messages",
        "baseUrl": "https://relay.example.com",
        "apiKey": "relay-key",
        "payload": {"model": "claude", "messages": []},
        "headers": {"anthropic-version": "2024-01-01", "x-trace": "abc"}
    }));
    assert_eq!(out["headers"]["anthropic-version"], json!("2024-01-01"));
    assert_eq!(out["headers"]["x-trace"], json!("abc"));
    assert_eq!(out["headers"]["x-api-key"], json!("relay-key"));
}

fn unified_prepare(request: &Value) -> Value {
    let text = CString::new(request.to_string()).expect("no interior nul");
    let envelope = lingxi_core::ffi::lingxi_unified_prepare(text.as_ptr());
    assert!(!envelope.is_null(), "prepare must always return an envelope");
    let body = unsafe { CStr::from_ptr(envelope) }.to_string_lossy().to_string();
    lingxi_core::ffi::lingxi_string_free(envelope);
    serde_json::from_str(&body).expect("envelope is JSON")
}

#[test]
fn unified_prepare_translates_lmp_into_native_wire() {
    let out = unified_prepare(&json!({
        "protocol": "anthropic_messages",
        "baseUrl": "https://api.anthropic.com",
        "apiKey": "k",
        "request": {
            "model": "claude-sonnet-4",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
            "maxTokens": 64
        },
        "stream": true
    }));
    assert_eq!(out["ok"], json!(true));
    assert_eq!(out["protocol"], json!("anthropic_messages"));
    assert_eq!(out["url"], json!("https://api.anthropic.com/v1/messages"));
    assert_eq!(out["body"]["stream"], json!(true));
    assert_eq!(out["body"]["max_tokens"], json!(64));
    assert_eq!(out["body"]["messages"][0]["content"][0]["text"], json!("hi"));
}

#[test]
fn unified_prepare_requires_an_api_key_for_bearer_protocols() {
    let out = unified_prepare(&json!({
        "protocol": "openai_chat_completions",
        "baseUrl": "https://api.example.com/v1",
        "request": {"model": "gpt-4o", "messages": []}
    }));
    assert_eq!(out["ok"], json!(false));
    assert_eq!(out["code"], json!("auth_missing"));
}

#[test]
fn unified_prepare_surfaces_unsupported_capabilities() {
    let out = unified_prepare(&json!({
        "protocol": "anthropic_messages",
        "baseUrl": "https://api.anthropic.com",
        "apiKey": "k",
        "request": {
            "model": "claude",
            "messages": [],
            "responseFormat": {"type": "json_schema", "name": "x", "schema": {}}
        }
    }));
    assert_eq!(out["ok"], json!(false));
    assert_eq!(out["code"], json!("unsupported_feature"));
}

#[test]
fn unified_prepare_rejects_unknown_protocol() {
    let out = unified_prepare(&json!({
        "protocol": "nope",
        "baseUrl": "https://example.com",
        "request": {"model": "m", "messages": []}
    }));
    assert_eq!(out["ok"], json!(false));
    assert_eq!(out["code"], json!("unsupported_protocol"));
}

