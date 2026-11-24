use std::ffi::{CStr, CString};
use std::path::PathBuf;

use lingxi_core::ffi::{
    lingxi_lmp_normalize_response, lingxi_lmp_translate_request, lingxi_string_free,
};
use lingxi_core::lmp::{
    ContentBlock, EmbeddingRequest, LmpError, LmpMessage, LmpRequest, LmpStreamEvent,
    RerankRequest, ResponseFormat, Role, ThinkingConfig, UnifiedClient,
};
use lingxi_core::NativeProtocol;
use serde_json::{json, Map, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn native_fixture(name: &str) -> Vec<u8> {
    std::fs::read(repo_root().join("conformance/native").join(name)).unwrap()
}

fn lmp_expectations() -> Value {
    let b = std::fs::read(repo_root().join("conformance/lmp/expectations.json")).unwrap();
    serde_json::from_slice(&b).unwrap()
}

async fn start_mock_http(status: u16, content_type: &'static str, body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let resp_body = body.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let head = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    resp_body.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(&resp_body).await;
            });
        }
    });
    format!("http://{addr}")
}

fn simple_req(model: &str) -> LmpRequest {
    LmpRequest {
        model: model.to_string(),
        messages: vec![LmpMessage {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "Hello".into(),
            }],
        }],
        max_tokens: Some(128),
        temperature: None,
        tools: Vec::new(),
        tool_choice: None,
        thinking: None,
        response_format: None,
        timeout_ms: None,
        provider_extensions: Map::new(),
    }
}

#[tokio::test]
async fn test_lmp_all_four_protocols_and_streaming() {
    let exp = lmp_expectations();
    let client = UnifiedClient::new();

    let cases = [
        (
            NativeProtocol::OpenaiChatCompletions,
            "openai_chat_completions",
            "openai_chat_response.json",
            "openai_chat_stream.sse",
            "gpt-5-mini",
        ),
        (
            NativeProtocol::OpenaiResponses,
            "openai_responses",
            "openai_responses_response.json",
            "openai_responses_stream.sse",
            "gpt-5",
        ),
        (
            NativeProtocol::AnthropicMessages,
            "anthropic_messages",
            "anthropic_messages_response.json",
            "anthropic_messages_stream.sse",
            "claude-sonnet-4-5",
        ),
        (
            NativeProtocol::GoogleGemini,
            "google_gemini",
            "google_gemini_response.json",
            "google_gemini_stream.sse",
            "gemini-2.5-pro",
        ),
    ];

    for (proto, key, resp_file, sse_file, model) in cases {
        let url = start_mock_http(200, "application/json", native_fixture(resp_file)).await;
        let resp = client
            .complete(proto, &url, Some("test-key".into()), &simple_req(model))
            .await
            .unwrap();
        let e = &exp["nonStreaming"][key];
        assert_eq!(resp.text(), e["text"].as_str().unwrap(), "{key} text");
        assert_eq!(
            resp.thinking_text(),
            e["thinking"].as_str().unwrap(),
            "{key} thinking"
        );
        assert_eq!(
            resp.finish_reason.as_deref(),
            e["finishReason"].as_str(),
            "{key} finishReason"
        );
        assert_eq!(
            resp.usage.input_tokens,
            e["inputTokens"].as_u64().unwrap(),
            "{key} inputTokens"
        );
        assert_eq!(
            resp.usage.output_tokens,
            e["outputTokens"].as_u64().unwrap(),
            "{key} outputTokens"
        );
        if let Some(cr) = e["cacheReadTokens"].as_u64() {
            assert_eq!(resp.usage.cache_read_tokens, Some(cr));
        }

        let surl = start_mock_http(200, "text/event-stream", native_fixture(sse_file)).await;
        let events = client
            .stream(proto, &surl, Some("test-key".into()), &simple_req(model))
            .await
            .unwrap();
        let streamed_text: String = events
            .iter()
            .filter_map(|ev| match ev {
                LmpStreamEvent::TextDelta { delta } => Some(delta.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            streamed_text,
            exp["streamingText"][key].as_str().unwrap(),
            "{key} streamingText"
        );
    }
}

#[tokio::test]
async fn test_lmp_explicit_unsupported_errors_and_c_abi() {
    let client = UnifiedClient::new();

    // Anthropic json_schema -> UnsupportedFeature (no silent downgrade)
    let mut bad_ant = simple_req("claude-sonnet-4-5");
    bad_ant.response_format = Some(ResponseFormat::JsonSchema {
        name: "out".into(),
        schema: json!({"type":"object"}),
        strict: Some(true),
    });
    let err = client
        .complete(
            NativeProtocol::AnthropicMessages,
            "https://api.anthropic.com",
            Some("k".into()),
            &bad_ant,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        LmpError::UnsupportedFeature { ref feature, .. } if feature == "json_schema"
    ));

    // OpenAI Chat budget_tokens -> UnsupportedFeature
    let mut bad_chat = simple_req("gpt-5-mini");
    bad_chat.thinking = Some(ThinkingConfig {
        effort: None,
        budget_tokens: Some(4096),
    });
    let err = client
        .complete(
            NativeProtocol::OpenaiChatCompletions,
            "https://api.openai.com/v1",
            Some("k".into()),
            &bad_chat,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        LmpError::UnsupportedFeature { ref feature, .. } if feature == "budget_tokens"
    ));

    // Embed & Rerank unsupported guards
    let emb_err = client
        .embed(
            NativeProtocol::AnthropicMessages,
            "https://api.anthropic.com",
            Some("k".into()),
            &EmbeddingRequest {
                model: "m".into(),
                input: vec!["hi".into()],
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(emb_err, LmpError::UnsupportedFeature { .. }));

    let rerank_err = client
        .rerank(
            NativeProtocol::GoogleGemini,
            &RerankRequest {
                model: "m".into(),
                query: "q".into(),
                documents: vec!["d".into()],
                top_n: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(rerank_err, LmpError::UnsupportedFeature { .. }));

    // C ABI round-trip verification
    let proto_c = CString::new("openai_chat_completions").unwrap();
    let req_json = serde_json::to_string(&simple_req("gpt-5-mini")).unwrap();
    let req_c = CString::new(req_json).unwrap();
    let translated_ptr = lingxi_lmp_translate_request(proto_c.as_ptr(), req_c.as_ptr());
    let translated_str = unsafe { CStr::from_ptr(translated_ptr) }.to_str().unwrap();
    let translated_val: Value = serde_json::from_str(translated_str).unwrap();
    assert_eq!(translated_val["ok"], true);
    assert_eq!(translated_val["payload"]["model"], "gpt-5-mini");
    lingxi_string_free(translated_ptr);

    let resp_c = CString::new(native_fixture("openai_chat_response.json")).unwrap();
    let norm_ptr = lingxi_lmp_normalize_response(proto_c.as_ptr(), resp_c.as_ptr());
    let norm_str = unsafe { CStr::from_ptr(norm_ptr) }.to_str().unwrap();
    let norm_val: Value = serde_json::from_str(norm_str).unwrap();
    assert_eq!(norm_val["ok"], true);
    assert_eq!(
        norm_val["response"]["usage"]["totalTokens"].as_u64(),
        Some(18)
    );
    lingxi_string_free(norm_ptr);
}
