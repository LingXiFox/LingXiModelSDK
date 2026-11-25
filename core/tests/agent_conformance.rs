//! Shared multi-turn / capability conformance suite.
//!
//! `conformance/agent` holds one request shape, one normalized LMP response
//! shape and one aggregated streamed shape per protocol. These tests are the
//! Rust reference implementation of that shared set; the language SDKs replay
//! the same fixtures through the C ABI.

use std::collections::BTreeMap;
use std::path::PathBuf;

use lingxi_core::lmp::adapters::{normalize_response, translate_request, StreamNormalizer};
use lingxi_core::lmp::{LmpRequest, LmpStreamEvent, UnifiedClient, Usage};
use lingxi_core::sse::SseEvent;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn load_json(relative: &str) -> Value {
    let path = repo_root().join(relative);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("cannot parse {}: {e}", path.display()))
}

fn expectations() -> Value {
    load_json("conformance/agent/expectations.json")
}

fn protocols() -> Vec<String> {
    expectations()["protocols"]
        .as_array()
        .expect("protocols")
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect()
}

fn protocol(name: &str) -> lingxi_core::NativeProtocol {
    lingxi_core::NativeProtocol::parse_str(name).unwrap_or_else(|| panic!("unknown protocol {name}"))
}

fn native_response(protocol: &str) -> Value {
    load_json(&format!("conformance/agent/responses/{protocol}.json"))
}

fn stream_events(protocol: &str) -> Vec<SseEvent> {
    let doc = load_json(&format!("conformance/agent/streams/{protocol}.json"));
    doc["events"]
        .as_array()
        .expect("events")
        .iter()
        .map(|event| SseEvent {
            event: event["event"].as_str().map(str::to_string),
            data: event["data"].as_str().unwrap().to_string(),
            id: event["id"].as_str().map(str::to_string),
        })
        .collect()
}

struct Aggregate {
    id: Option<String>,
    text: String,
    thinking: String,
    tool_calls: BTreeMap<usize, ToolCall>,
    signatures: Vec<(String, String)>,
    finish_reason: Option<String>,
    usage: Option<Usage>,
}

#[derive(Default)]
struct ToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

fn aggregate(events: &[LmpStreamEvent]) -> Aggregate {
    let mut out = Aggregate {
        id: None,
        text: String::new(),
        thinking: String::new(),
        tool_calls: BTreeMap::new(),
        signatures: Vec::new(),
        finish_reason: None,
        usage: None,
    };
    for event in events {
        match event {
            LmpStreamEvent::MessageStart { id } => {
                if out.id.is_none() {
                    out.id = id.clone();
                }
            }
            LmpStreamEvent::ThinkingDelta { delta, signature } => {
                out.thinking.push_str(delta);
                if let Some(sig) = signature {
                    out.signatures.push(("thinking".into(), sig.clone()));
                }
            }
            LmpStreamEvent::TextDelta { delta, signature } => {
                out.text.push_str(delta);
                if let Some(sig) = signature {
                    out.signatures.push(("text".into(), sig.clone()));
                }
            }
            LmpStreamEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
                signature,
            } => {
                let call = out.tool_calls.entry(*index).or_default();
                if id.is_some() {
                    call.id = id.clone();
                }
                if name.is_some() {
                    call.name = name.clone();
                }
                call.arguments.push_str(arguments_delta);
                if let Some(sig) = signature {
                    out.signatures.push(("tool_call".into(), sig.clone()));
                }
            }
            LmpStreamEvent::Usage { usage } => out.usage = Some(usage.clone()),
            LmpStreamEvent::MessageEnd { finish_reason } => out.finish_reason = finish_reason.clone(),
            LmpStreamEvent::Error { code, message } => {
                panic!("unexpected terminal stream error {code}: {message}")
            }
        }
    }
    out
}

fn run_stream(protocol_name: &str, events: &[SseEvent]) -> Vec<LmpStreamEvent> {
    let mut normalizer = StreamNormalizer::new(protocol(protocol_name));
    let mut out = Vec::new();
    for event in events {
        normalizer
            .push(event, &mut out)
            .unwrap_or_else(|e| panic!("{protocol_name} push failed: {e}"));
    }
    normalizer.finish(&mut out);
    out
}

fn usage_json(usage: &Usage) -> Value {
    serde_json::to_value(usage).unwrap()
}

/// Every protocol must normalize the published fixtures to the same LMP
/// response: text, reasoning, parallel tool calls, exact usage, cache counters.
#[test]
fn normalized_responses_match_shared_expectations() {
    let expected = expectations();
    for protocol_name in protocols() {
        let expected = &expected["responses"][&protocol_name];
        assert!(!expected.is_null(), "missing response expectation for {protocol_name}");
        let response = normalize_response(protocol(&protocol_name), &native_response(&protocol_name))
            .unwrap_or_else(|e| panic!("{protocol_name} normalize failed: {e}"));

        assert_eq!(response.text(), expected["text"].as_str().unwrap(), "{protocol_name} text");
        assert_eq!(
            response.thinking_text(),
            expected["thinking"].as_str().unwrap(),
            "{protocol_name} thinking"
        );
        assert_eq!(
            response.finish_reason.as_deref(),
            expected["finishReason"].as_str(),
            "{protocol_name} finish reason"
        );

        let thinking_signature = response
            .content
            .iter()
            .find_map(|block| match block {
                lingxi_core::lmp::ContentBlock::Thinking { signature, .. } => signature.clone(),
                _ => None,
            });
        assert_eq!(
            thinking_signature.as_deref(),
            expected["thinkingSignature"].as_str(),
            "{protocol_name} thinking signature"
        );

        let calls: Vec<Value> = response
            .content
            .iter()
            .filter_map(|block| match block {
                lingxi_core::lmp::ContentBlock::ToolCall {
                    id, name, arguments, ..
                } => Some(json!({ "id": id, "name": name, "arguments": arguments })),
                _ => None,
            })
            .collect();
        assert_eq!(calls, expected["toolCalls"].as_array().unwrap().clone(), "{protocol_name} tool calls");

        let order: Vec<&str> = response
            .content
            .iter()
            .map(|block| match block {
                lingxi_core::lmp::ContentBlock::Thinking { .. } => "thinking",
                lingxi_core::lmp::ContentBlock::Text { .. } => "text",
                lingxi_core::lmp::ContentBlock::ToolCall { .. } => "tool_call",
                _ => "other",
            })
            .collect();
        let expected_order: Vec<&str> = expected["contentOrder"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(order, expected_order, "{protocol_name} content order");
        assert_eq!(usage_json(&response.usage), expected["usage"], "{protocol_name} usage");
    }
}

/// Streaming must aggregate to the same semantic shape as the non-streaming
/// fixture: text, reasoning, parallel tool calls with fragment concatenation,
/// opaque reasoning signatures, finish reason and exact usage.
#[test]
fn streamed_aggregates_match_shared_expectations() {
    let expected = expectations();
    for protocol_name in protocols() {
        let want = &expected["streams"][&protocol_name];
        assert!(!want.is_null(), "missing stream expectation for {protocol_name}");
        let events = run_stream(&protocol_name, &stream_events(&protocol_name));
        let got = aggregate(&events);

        assert_eq!(got.text, want["text"].as_str().unwrap(), "{protocol_name} stream text");
        assert_eq!(got.thinking, want["thinking"].as_str().unwrap(), "{protocol_name} stream thinking");
        assert_eq!(
            got.id.as_deref(),
            want["id"].as_str(),
            "{protocol_name} stream id"
        );
        assert_eq!(
            got.finish_reason.as_deref(),
            want["finishReason"].as_str(),
            "{protocol_name} stream finish reason"
        );

        let calls: Vec<Value> = got
            .tool_calls
            .iter()
            .map(|(index, call)| {
                let arguments: Value = if call.arguments.is_empty() {
                    Value::Null
                } else {
                    serde_json::from_str(&call.arguments)
                        .unwrap_or_else(|e| panic!("{protocol_name} arguments not JSON: {e}"))
                };
                json!({
                    "index": index,
                    "id": call.id,
                    "name": call.name,
                    "arguments": arguments,
                })
            })
            .collect();
        assert_eq!(calls, want["toolCalls"].as_array().unwrap().clone(), "{protocol_name} streamed tool calls");

        let signatures: Vec<Value> = got
            .signatures
            .iter()
            .map(|(kind, signature)| json!([kind, signature]))
            .collect();
        assert_eq!(
            signatures,
            want["signatures"].as_array().unwrap().clone(),
            "{protocol_name} streamed signatures"
        );

        let usage = got.usage.expect("stream usage");
        assert_eq!(usage_json(&usage), want["usage"], "{protocol_name} stream usage");
    }
}

/// A single request shape must translate on every protocol, and the recorded
/// unsupported-only cells must stay unsupported with a typed error code.
#[test]
fn translations_match_shared_snapshot() {
    let snapshot = load_json("conformance/agent/translations.json");
    let mut checked = 0;
    for (request_id, per_protocol) in snapshot["requests"].as_object().unwrap() {
        let request: LmpRequest =
            serde_json::from_value(load_json(&format!("conformance/agent/requests/{request_id}.json"))["request"].clone())
                .unwrap_or_else(|e| panic!("{request_id} is not an LMP request: {e}"));
        for (protocol_name, expected) in per_protocol.as_object().unwrap() {
            let result = translate_request(protocol(protocol_name), &request);
            match (result, expected.get("payload")) {
                (Ok(payload), Some(want)) => {
                    assert_eq!(&payload, want, "{request_id} / {protocol_name} payload");
                }
                (Err(err), None) => {
                    let want = &expected["error"];
                    assert_eq!(err.code(), want["code"].as_str().unwrap(), "{request_id} / {protocol_name} code");
                    assert_eq!(err.to_string(), want["message"].as_str().unwrap(), "{request_id} / {protocol_name} message");
                }
                (Ok(payload), None) => panic!("{request_id} / {protocol_name} expected an error, translated to {payload}"),
                (Err(err), Some(_)) => panic!("{request_id} / {protocol_name} expected a payload, got error: {err}"),
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 27, "every protocol must cover every request");
}

/// Replaying a normalized assistant turn (with opaque reasoning signatures) and
/// its tool results must keep every call id and every result: this is the
/// regression that used to collapse parallel tool results into the last one.
#[test]
fn multi_turn_tool_replay_keeps_every_call_and_result() {
    let tools = load_json("conformance/agent/requests/multi-turn-tools.json")["request"]["tools"].clone();
    for protocol_name in protocols() {
        let normalized = normalize_response(protocol(&protocol_name), &native_response(&protocol_name))
            .unwrap_or_else(|e| panic!("{protocol_name} normalize failed: {e}"));
        let assistant_content = serde_json::to_value(&normalized.content).unwrap();
        let call_ids: Vec<String> = normalized
            .content
            .iter()
            .filter_map(|block| match block {
                lingxi_core::lmp::ContentBlock::ToolCall { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(call_ids.len(), 2, "{protocol_name} fixture must expose two calls");

        let request: LmpRequest = serde_json::from_value(json!({
            "model": "mock-1",
            "messages": [
                { "role": "user", "content": [{ "type": "text", "text": "Compare Tokyo and Osaka." }] },
                { "role": "assistant", "content": assistant_content },
                { "role": "tool", "content": [{ "type": "tool_result", "toolCallId": call_ids[0], "content": "18 C" }] },
                { "role": "tool", "content": [{ "type": "tool_result", "toolCallId": call_ids[1], "content": "21 C" }] }
            ],
            "tools": tools,
        }))
        .unwrap();

        let payload = translate_request(protocol(&protocol_name), &request)
            .unwrap_or_else(|e| panic!("{protocol_name} replay failed: {e}"));
        let text = payload.to_string();
        assert!(text.contains("18 C"), "{protocol_name} dropped the first tool result");
        assert!(text.contains("21 C"), "{protocol_name} dropped the second tool result");

        // Ollama associates results by function name rather than id, so its
        // synthesized ids are not replayed; every other protocol keeps both.
        if protocol_name != "ollama" {
            for call_id in &call_ids {
                assert!(
                    text.contains(call_id.as_str()),
                    "{protocol_name} dropped call id {call_id}"
                );
            }
        }
        assert!(
            text.contains("get_weather"),
            "{protocol_name} dropped the tool definition"
        );
    }
}

/// Bedrock's binary event stream must reach the LMP layer intact when consumed
/// through the real HTTP path (not just the decoded fixture).
#[tokio::test]
async fn bedrock_event_stream_reaches_lmp_through_http() {
    let blob = std::fs::read(repo_root().join("conformance/agent/streams/aws_bedrock.eventstream")).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let _ = socket.read(&mut buf).await;
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.amazon.eventstream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            blob.len()
        );
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(&blob).await.unwrap();
    });

    let request = load_json("conformance/agent/requests/multi-turn-tools.json")["request"].clone();
    let request: LmpRequest = serde_json::from_value(request).unwrap();
    let events = UnifiedClient::new()
        .stream(
            protocol("aws_bedrock"),
            &format!("http://{addr}"),
            Some("AKID:SECRET".into()),
            &request,
        )
        .await
        .expect("bedrock stream");
    let got = aggregate(&events);
    assert_eq!(got.thinking, "I need both readings.");
    assert_eq!(got.text, "Checking both.");
    assert_eq!(got.tool_calls.len(), 2);
    assert_eq!(
        got.tool_calls[&2].arguments,
        "{\"city\":\"Tokyo\"}",
        "bedrock must concatenate streamed tool input"
    );
    assert_eq!(got.signatures, vec![("thinking".to_string(), "sig-think-1".to_string())]);
    assert_eq!(got.finish_reason.as_deref(), Some("tool_use"));
}

/// Structured output and multimodal input are translated only where the
/// protocol really supports them; everything else must raise a typed error
/// instead of silently dropping content.
#[test]
fn multimodal_and_structured_errors_are_typed() {
    let snapshot = load_json("conformance/agent/translations.json");
    let expected = &snapshot["requests"]["multimodal-structured"];
    let request: LmpRequest =
        serde_json::from_value(load_json("conformance/agent/requests/multimodal-structured.json")["request"].clone())
            .unwrap();

    for protocol_name in protocols() {
        let result = translate_request(protocol(&protocol_name), &request);
        if expected[&protocol_name].get("payload").is_some() {
            let payload = result.unwrap_or_else(|e| panic!("{protocol_name} should support structured output: {e}"));
            let text = payload.to_string();
            assert!(
                text.contains("receipt") || text.contains("json_schema") || text.contains("jsonSchema"),
                "{protocol_name} structured output was dropped: {text}"
            );
        } else {
            let err = result.expect_err(&format!("{protocol_name} must reject structured output"));
            assert_eq!(
                err.code(),
                expected[&protocol_name]["error"]["code"].as_str().unwrap(),
                "{protocol_name} error code"
            );
        }
    }
}

#[test]
fn forced_tool_choice_is_translated_or_rejected_explicitly() {
    let snapshot = load_json("conformance/agent/translations.json");
    let expected = &snapshot["requests"]["forced-tool"];
    let request: LmpRequest =
        serde_json::from_value(load_json("conformance/agent/requests/forced-tool.json")["request"].clone()).unwrap();
    for protocol_name in protocols() {
        let result = translate_request(protocol(&protocol_name), &request);
        match expected[&protocol_name].get("payload") {
            Some(payload) => {
                let got = result.unwrap_or_else(|e| panic!("{protocol_name} should force the tool: {e}"));
                assert_eq!(&got, payload, "{protocol_name} forced tool choice payload");
            }
            None => {
                let err = result.expect_err(&format!("{protocol_name} must reject forced tool choice"));
                assert_eq!(
                    err.code(),
                    expected[&protocol_name]["error"]["code"].as_str().unwrap(),
                    "{protocol_name} forced tool choice code"
                );
            }
        }
    }
}
