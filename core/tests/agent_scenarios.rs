use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use lingxi_core::lmp::{
    ContentBlock, EmbeddingRequest, LmpError, LmpMessage, LmpRequest, LmpStreamEvent, RerankRequest,
    ResponseFormat, Role, ThinkingConfig, UnifiedClient,
};
use lingxi_core::NativeProtocol;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

async fn start_scripted_server<B: AsRef<[u8]> + Send + Sync>(
    handler: impl Fn(String) -> (u16, &'static str, B) + Send + Sync + 'static,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handler = Arc::new(handler);
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            stream.set_nodelay(true).unwrap();
            let h = Arc::clone(&handler);
            tokio::spawn(async move {
                let (reader, mut writer) = stream.split();
                let mut reader = BufReader::new(reader);
                loop {
                    let mut req = String::new();
                    let mut length = 0;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                            return;
                        }
                        req.push_str(&line);
                        if line == "\r\n" {
                            break;
                        }
                        if let Some((name, value)) = line.split_once(':') {
                            if name.eq_ignore_ascii_case("content-length") {
                                length = value.trim().parse().unwrap();
                            }
                        }
                    }
                    let mut body = vec![0; length];
                    if reader.read_exact(&mut body).await.is_err() {
                        return;
                    }
                    req.push_str(std::str::from_utf8(&body).unwrap());
                    let (status, ctype, body) = h(req);
                    let body = body.as_ref();
                    let resp = format!(
                        "HTTP/1.1 {status} OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
                        body.len()
                    );
                    if writer.write_all(resp.as_bytes()).await.is_err() || writer.write_all(body).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    (format!("http://127.0.0.1:{}", addr.port()), handle)
}

#[tokio::test]
async fn tier2_protocols_complete_and_stream_against_mock_servers() {
    let client = UnifiedClient::new();

    // 1. AWS Bedrock (SigV4 + Converse + Amazon Event Stream)
    let bedrock_resp = include_str!("../../conformance/native/aws_bedrock_response.json").to_string();
    let bedrock_stream = include_bytes!("../../conformance/native/aws_bedrock_stream.eventstream");
    let (base, h1) = start_scripted_server(move |req| {
        assert!(req.contains("AWS4-HMAC-SHA256"), "missing SigV4 header: {req}");
        if req.contains("converse-stream") {
            (200, "application/vnd.amazon.eventstream", bedrock_stream.to_vec())
        } else {
            (200, "application/json", bedrock_resp.as_bytes().to_vec())
        }
    })
    .await;
    let req = LmpRequest {
        model: "anthropic.claude-3-5-sonnet".into(),
        messages: vec![LmpMessage {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hi".into(), signature: None }],
        }],
        max_tokens: Some(128),
        temperature: None,
        tools: vec![],
        tool_choice: None,
        thinking: Some(ThinkingConfig {
            effort: None,
            budget_tokens: Some(2048),
        }),
        response_format: None,
        timeout_ms: Some(5000),
        provider_extensions: Default::default(),
    };
    let out = client
        .complete(
            NativeProtocol::AwsBedrock,
            &base,
            Some("AKIAEXAMPLE:SECRETEXAMPLE".into()),
            &req,
        )
        .await
        .unwrap();
    assert!(out.content.iter().any(|b| matches!(b, ContentBlock::Thinking { signature: Some(s), .. } if s == "sig-bedrock-1")));
    let events = client
        .stream(
            NativeProtocol::AwsBedrock,
            &base,
            Some("AKIAEXAMPLE:SECRETEXAMPLE".into()),
            &req,
        )
        .await
        .unwrap();
    assert!(events.iter().any(|e| matches!(e, LmpStreamEvent::TextDelta { delta } if delta == "Hello ")));
    h1.abort();

    // 2. Google Vertex AI (thoughtSignature preservation + SSE)
    let vertex_resp = include_str!("../../conformance/native/google_vertex_response.json").to_string();
    let vertex_stream = include_str!("../../conformance/native/google_vertex_stream.sse").to_string();
    let (base, h2) = start_scripted_server(move |req| {
        assert!(req.contains("Authorization: Bearer vertex-tok") || req.contains("authorization: Bearer vertex-tok"));
        if req.contains("streamGenerateContent") {
            (200, "text/event-stream", vertex_stream.clone())
        } else {
            (200, "application/json", vertex_resp.clone())
        }
    })
    .await;
    let out = client
        .complete(
            NativeProtocol::GoogleVertexAi,
            &base,
            Some("vertex-tok".into()),
            &req,
        )
        .await
        .unwrap();
    assert!(out.content.iter().any(|b| matches!(b, ContentBlock::Thinking { signature: Some(s), .. } if s == "sig-vertex-1")));
    let events = client
        .stream(
            NativeProtocol::GoogleVertexAi,
            &base,
            Some("vertex-tok".into()),
            &req,
        )
        .await
        .unwrap();
    assert!(events.iter().any(|e| matches!(e, LmpStreamEvent::TextDelta { .. })));
    h2.abort();

    // 3. Azure OpenAI
    let azure_resp = include_str!("../../conformance/native/azure_openai_response.json").to_string();
    let (base, h3) = start_scripted_server(move |req| {
        assert!(req.contains("/openai/deployments/gpt-4o/chat/completions?api-version="));
        assert!(req.contains("api-key: az-key") || req.contains("api-key:az-key"));
        (200, "application/json", azure_resp.clone())
    })
    .await;
    let mut az_req = req.clone();
    az_req.model = "gpt-4o".into();
    az_req.thinking = None;
    let out = client
        .complete(NativeProtocol::AzureOpenai, &base, Some("az-key".into()), &az_req)
        .await
        .unwrap();
    assert!(out.content.iter().any(|b| matches!(b, ContentBlock::Text { text, .. } if text.contains("Azure"))));
    h3.abort();

    // 4. Ollama + explicit unsupported strict json_schema
    let ollama_resp = include_str!("../../conformance/native/ollama_chat_response.json").to_string();
    let ollama_stream = include_str!("../../conformance/native/ollama_chat_stream.ndjson").to_string();
    let (base, h4) = start_scripted_server(move |req| {
        if req.contains("\"stream\":true") {
            (200, "application/x-ndjson", ollama_stream.clone())
        } else {
            (200, "application/json", ollama_resp.clone())
        }
    })
    .await;
    let mut ol_req = az_req.clone();
    ol_req.model = "llama3.3".into();
    let out = client
        .complete(NativeProtocol::Ollama, &base, None, &ol_req)
        .await
        .unwrap();
    assert_eq!(out.usage.total_tokens, 22);
    let events = client
        .stream(NativeProtocol::Ollama, &base, None, &ol_req)
        .await
        .unwrap();
    assert!(events.iter().any(|e| matches!(e, LmpStreamEvent::TextDelta { delta } if delta == "Ollama")));
    ol_req.response_format = Some(ResponseFormat::JsonSchema {
        name: "x".into(),
        schema: json!({"type": "object"}),
        strict: Some(true),
    });
    let err = client
        .complete(NativeProtocol::Ollama, &base, None, &ol_req)
        .await
        .unwrap_err();
    assert!(matches!(err, LmpError::UnsupportedFeature { .. }));
    h4.abort();

    // 5. Cohere Chat + Rerank + Embed
    let cohere_resp = include_str!("../../conformance/native/cohere_chat_response.json").to_string();
    let cohere_rerank = include_str!("../../conformance/native/cohere_rerank_response.json").to_string();
    let (base, h5) = start_scripted_server(move |req| {
        if req.contains("/v2/rerank") {
            (200, "application/json", cohere_rerank.clone())
        } else {
            (200, "application/json", cohere_resp.clone())
        }
    })
    .await;
    let out = client
        .complete(NativeProtocol::Cohere, &base, Some("co-key".into()), &az_req)
        .await
        .unwrap();
    assert!(out.content.iter().any(|b| matches!(b, ContentBlock::Text { text, .. } if text.contains("Cohere"))));
    let rr = client
        .rerank(
            NativeProtocol::Cohere,
            &base,
            Some("co-key".into()),
            &RerankRequest {
                model: "rerank-v3.5".into(),
                query: "rust".into(),
                documents: vec!["a".into(), "b".into()],
                top_n: Some(2),
            },
        )
        .await
        .unwrap();
    assert_eq!(rr.results.len(), 2);
    assert_eq!(rr.results[0].index, 1);
    h5.abort();
}

#[tokio::test]
async fn real_embed_and_redirect_rejection_and_200_reuse() {
    let client = UnifiedClient::new();

    // Real OpenAI embed via HTTP
    let embed_resp = include_str!("../../conformance/native/openai_embed_response.json").to_string();
    let (base, h1) = start_scripted_server(move |_req| {
        (200, "application/json", embed_resp.clone())
    })
    .await;
    let emb = client
        .embed(
            NativeProtocol::OpenaiChatCompletions,
            &base,
            Some("sk-test".into()),
            &EmbeddingRequest {
                model: "text-embedding-3-small".into(),
                input: vec!["hello".into(), "world".into()],
            },
        )
        .await
        .unwrap();
    assert_eq!(emb.embeddings.len(), 2);
    h1.abort();

    // Redirect rejection (302 must not follow or leak auth)
    let (redir_base, h2) = start_scripted_server(move |_req| {
        (302, "text/plain", "redirect blocked")
    })
    .await;
    let req = LmpRequest {
        model: "gpt-4o".into(),
        messages: vec![LmpMessage {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hi".into(), signature: None }],
        }],
        max_tokens: None,
        temperature: None,
        tools: vec![],
        tool_choice: None,
        thinking: None,
        response_format: None,
        timeout_ms: Some(3000),
        provider_extensions: Default::default(),
    };
    let err = client
        .complete(NativeProtocol::OpenaiChatCompletions, &redir_base, Some("sk-secret".into()), &req)
        .await
        .unwrap_err();
    assert!(!matches!(err, LmpError::UnsupportedFeature { .. }));
    h2.abort();

    // 200 sequential keep-alive requests
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_c = Arc::clone(&hits);
    let chat_resp = include_str!("../../conformance/native/openai_chat_response.json").to_string();
    let (keep_base, h3) = start_scripted_server(move |_req| {
        hits_c.fetch_add(1, Ordering::Relaxed);
        (200, "application/json", chat_resp.clone())
    })
    .await;
    for _ in 0..200 {
        let res = client
            .complete(NativeProtocol::OpenaiChatCompletions, &keep_base, Some("sk-keep".into()), &req)
            .await
            .unwrap();
        assert!(!res.content.is_empty());
    }
    assert_eq!(hits.load(Ordering::Relaxed), 200);
    h3.abort();
}

#[tokio::test]
async fn gemini_replays_parallel_tool_calls_and_original_signed_parts() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!("../../conformance/agent/gemini-tool-loop.json")).unwrap();
    let mocked = fixture.clone();
    let (base, server) = start_scripted_server(move |request| {
        let (_, body) = request.split_once("\r\n\r\n").unwrap();
        let payload: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["cachedContent"], "cachedContents/mock-1");
        assert_eq!(payload["generationConfig"]["thinkingConfig"], json!({"thinkingLevel": "LOW", "includeThoughts": true}));
        assert_eq!(payload["contents"][0]["parts"][1]["inlineData"]["mimeType"], "image/png");
        if payload["contents"].as_array().unwrap().len() == 1 {
            (200, "application/json", mocked["firstResponse"].to_string())
        } else {
            assert_eq!(payload["contents"][1], mocked["firstResponse"]["candidates"][0]["content"]);
            assert_eq!(payload["contents"][2]["parts"], mocked["expectedToolResponses"]);
            (200, "application/json", mocked["finalResponse"].to_string())
        }
    }).await;
    let client = UnifiedClient::new();
    let mut req: LmpRequest = serde_json::from_value(fixture["request"].clone()).unwrap();
    let first = client.complete(NativeProtocol::GoogleGemini, &base, Some("fixture".into()), &req).await.unwrap();
    assert_eq!(first.usage.cache_read_tokens, Some(0));
    assert_eq!(first.usage.reasoning_tokens, Some(5));
    let results: Vec<_> = first.content.iter().filter_map(|block| match block {
        ContentBlock::ToolCall { id, arguments, .. } => Some(LmpMessage {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_call_id: id.clone(),
                content: if arguments["city"] == "東京" { "18 C" } else { "21 C" }.into(),
                is_error: false,
            }],
        }),
        _ => None,
    }).collect();
    assert_eq!(results.len(), 2);
    req.messages.push(LmpMessage { role: Role::Assistant, content: first.content });
    req.messages.extend(results);
    let last = client.complete(NativeProtocol::GoogleGemini, &base, Some("fixture".into()), &req).await.unwrap();
    assert_eq!(last.text(), "Osaka is 3 C warmer.");
    assert_eq!(last.usage.cache_read_tokens, Some(25));
    server.abort();
}

#[tokio::test]
async fn gemini_embedding_sends_all_inputs_and_validates_result_count() {
    let (base, server) = start_scripted_server(|request| {
        assert!(request.starts_with("POST /v1beta/models/mock:batchEmbedContents "));
        let body: serde_json::Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["requests"][0]["content"]["parts"][0]["text"], "first");
        assert_eq!(body["requests"][1]["content"]["parts"][0]["text"], "second");
        assert_eq!(body["requests"][1]["model"], "models/mock");
        (200, "application/json", r#"{"embeddings":[{"values":[0.1]},{"values":[0.2]}]}"#)
    }).await;
    let request = EmbeddingRequest { model: "models/mock".into(), input: vec!["first".into(), "second".into()] };
    let client = UnifiedClient::new();
    let response = client.embed(NativeProtocol::GoogleGemini, &base, Some("fixture".into()), &request).await.unwrap();
    assert_eq!(response.embeddings, vec![vec![0.1], vec![0.2]]);
    assert!(matches!(client.embed(NativeProtocol::GoogleVertexAi, &base, Some("fixture".into()), &request).await, Err(LmpError::UnsupportedFeature { .. })));
    server.abort();
}
