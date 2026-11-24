use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use lingxi_core::{NativeClient, NativeError, NativeProtocol, NativeRequestConfig, ProviderRegistry};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(repo_root().join("conformance/native").join(name)).unwrap()
}

#[derive(Debug, Clone)]
struct RecordedReq {
    target: String,
    headers: String,
    body: String,
}

async fn start_mock_server(
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
) -> (String, Arc<Mutex<Vec<RecordedReq>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let rec_clone = recorded.clone();

    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let rec = rec_clone.clone();
            let resp_body = body.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                let mut header_end = None;
                let mut content_len = 0usize;
                loop {
                    let n = stream.read(&mut tmp).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if header_end.is_none() {
                        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            header_end = Some(pos + 4);
                            let head_str = String::from_utf8_lossy(&buf[..pos]);
                            for line in head_str.lines() {
                                if let Some((k, v)) = line.split_once(':') {
                                    if k.eq_ignore_ascii_case("content-length") {
                                        content_len = v.trim().parse().unwrap_or(0);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(end) = header_end {
                        if buf.len() >= end + content_len {
                            break;
                        }
                    }
                }
                let end = header_end.unwrap_or(buf.len());
                let head = String::from_utf8_lossy(&buf[..end]).to_string();
                let target = head
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .unwrap_or("")
                    .to_string();
                let req_body = String::from_utf8_lossy(&buf[end..]).to_string();
                rec.lock().unwrap().push(RecordedReq {
                    target,
                    headers: head.to_lowercase(),
                    body: req_body,
                });

                let resp_head = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    resp_body.len()
                );
                let _ = stream.write_all(resp_head.as_bytes()).await;
                let _ = stream.write_all(&resp_body).await;
            });
        }
    });

    (format!("http://{addr}"), recorded)
}

#[tokio::test]
async fn test_four_native_protocols_and_streaming() {
    let client = NativeClient::new();

    // 1. OpenAI Chat Completions (JSON + SSE + passthrough extension)
    let (url, rec) =
        start_mock_server(200, "application/json", fixture("openai_chat_response.json")).await;
    let cfg = NativeRequestConfig::new(format!("{url}/v1"), Some("sk-openai".into()));
    let resp = client
        .openai_chat_completions(
            &cfg,
            &json!({"model":"gpt-5-mini","messages":[{"role":"user","content":"hi"}],"custom_ext":42}),
        )
        .await
        .unwrap();
    assert_eq!(resp.body["id"], "chatcmpl-mock-1");
    assert_eq!(resp.body["choices"][0]["message"]["reasoning_content"], "step-by-step");
    let reqs = rec.lock().unwrap();
    assert_eq!(reqs[0].target, "/v1/chat/completions");
    assert!(reqs[0].headers.contains("authorization: bearer sk-openai"));
    assert!(reqs[0].body.contains("\"custom_ext\":42"));

    let (surl, _) =
        start_mock_server(200, "text/event-stream", fixture("openai_chat_stream.sse")).await;
    let events = client
        .stream_openai_chat_completions(
            &NativeRequestConfig::new(format!("{surl}/v1"), Some("sk-openai".into())),
            &json!({"model":"gpt-5-mini","messages":[]}),
        )
        .await
        .unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[2].data, "[DONE]");

    // 2. OpenAI Responses (JSON + typed SSE)
    let (url, rec) =
        start_mock_server(200, "application/json", fixture("openai_responses_response.json")).await;
    let resp = client
        .openai_responses(
            &NativeRequestConfig::new(format!("{url}/v1"), Some("sk-resp".into())),
            &json!({"model":"gpt-5","input":"hi"}),
        )
        .await
        .unwrap();
    assert_eq!(resp.body["id"], "resp-mock-1");
    assert_eq!(rec.lock().unwrap()[0].target, "/v1/responses");

    let (surl, _) =
        start_mock_server(200, "text/event-stream", fixture("openai_responses_stream.sse")).await;
    let events = client
        .stream_openai_responses(
            &NativeRequestConfig::new(format!("{surl}/v1"), Some("sk-resp".into())),
            &json!({"model":"gpt-5","input":"hi"}),
        )
        .await
        .unwrap();
    assert_eq!(events.len(), 4);
    assert_eq!(events[1].event.as_deref(), Some("response.output_text.delta"));

    // 3. Anthropic Messages (JSON + typed SSE + x-api-key + anthropic-version)
    let (url, rec) =
        start_mock_server(200, "application/json", fixture("anthropic_messages_response.json")).await;
    let resp = client
        .anthropic_messages(
            &NativeRequestConfig::new(url.clone(), Some("sk-ant".into())),
            &json!({"model":"claude-sonnet-4-5","max_tokens":1024,"messages":[{"role":"user","content":"hi"}]}),
        )
        .await
        .unwrap();
    assert_eq!(resp.body["id"], "msg_mock_1");
    {
        let r = rec.lock().unwrap();
        assert_eq!(r[0].target, "/v1/messages");
        assert!(r[0].headers.contains("x-api-key: sk-ant"));
        assert!(r[0].headers.contains("anthropic-version: 2023-06-01"));
    }

    let (surl, _) =
        start_mock_server(200, "text/event-stream", fixture("anthropic_messages_stream.sse")).await;
    let events = client
        .stream_anthropic_messages(
            &NativeRequestConfig::new(surl, Some("sk-ant".into())),
            &json!({"model":"claude-sonnet-4-5","max_tokens":1024,"messages":[]}),
        )
        .await
        .unwrap();
    assert_eq!(events.len(), 4);
    assert_eq!(events[1].event.as_deref(), Some("content_block_delta"));
    assert!(events[1].data.contains("thinking_delta"));

    // 4. Google Gemini (JSON + SSE + x-goog-api-key)
    let (url, rec) =
        start_mock_server(200, "application/json", fixture("google_gemini_response.json")).await;
    let resp = client
        .google_gemini_generate(
            &NativeRequestConfig::new(url.clone(), Some("gem-key".into())),
            "gemini-2.5-pro",
            &json!({"contents":[{"parts":[{"text":"hi"}]}]}),
        )
        .await
        .unwrap();
    assert_eq!(resp.body["candidates"][0]["finishReason"], "STOP");
    {
        let r = rec.lock().unwrap();
        assert_eq!(r[0].target, "/v1beta/models/gemini-2.5-pro:generateContent");
        assert!(r[0].headers.contains("x-goog-api-key: gem-key"));
    }

    let (surl, rec) =
        start_mock_server(200, "text/event-stream", fixture("google_gemini_stream.sse")).await;
    let events = client
        .stream_google_gemini_generate(
            &NativeRequestConfig::new(surl, Some("gem-key".into())),
            "gemini-2.5-pro",
            &json!({"contents":[]}),
        )
        .await
        .unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(
        rec.lock().unwrap()[0].target,
        "/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse"
    );
}

#[tokio::test]
async fn test_registry_dispatch_error_passthrough_and_security() {
    let reg_bytes = std::fs::read(repo_root().join("registry/registry.json")).unwrap();
    let registry = ProviderRegistry::from_json(&reg_bytes).unwrap();
    assert!(registry.len() >= 300);

    // Discovered provider refuses protocol dispatch
    let err = registry
        .resolve_endpoint("fiftyx", NativeProtocol::OpenaiChatCompletions, None, None, false)
        .unwrap_err();
    assert!(matches!(err, NativeError::UnsupportedProtocol { .. }));

    // Non-HTTPS non-loopback endpoint rejected
    let sec_err = registry
        .resolve_endpoint(
            "openai",
            NativeProtocol::OpenaiChatCompletions,
            None,
            Some("http://evil.example.com/v1"),
            false,
        )
        .unwrap_err();
    assert!(matches!(sec_err, NativeError::InvalidEndpoint(_)));

    // Declarative dispatch via DeepSeek entry + 429 error body preservation
    let (url, rec) = start_mock_server(429, "application/json", fixture("error_429.json")).await;
    let client = NativeClient::new();
    let err = client
        .send_registered_json(
            &registry,
            "deepseek",
            NativeProtocol::OpenaiChatCompletions,
            Some("deepseek-reasoner"),
            Some("sk-ds".into()),
            Some(&url),
            &json!({"model":"deepseek-reasoner","messages":[]}),
        )
        .await
        .unwrap_err();
    match err {
        NativeError::HttpStatus { status, body, .. } => {
            assert_eq!(status, 429);
            assert!(String::from_utf8_lossy(&body).contains("rate_limit_exceeded"));
        }
        other => panic!("expected HttpStatus 429, got {other:?}"),
    }
    assert_eq!(rec.lock().unwrap()[0].target, "/chat/completions");
}
