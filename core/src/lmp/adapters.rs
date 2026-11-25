use std::collections::HashMap;

use serde_json::{json, Map, Value};

use crate::lmp::errors::LmpError;
use crate::lmp::types::{
    ContentBlock, LmpRequest, LmpResponse, LmpStreamEvent, ResponseFormat, Role, ToolChoice, Usage,
};
use crate::registry::NativeProtocol;
use crate::sse::SseEvent;

pub fn translate_request(protocol: NativeProtocol, req: &LmpRequest) -> Result<Value, LmpError> {
    let mut payload = match protocol {
        NativeProtocol::OpenaiChatCompletions | NativeProtocol::AzureOpenai => to_openai_chat(req)?,
        NativeProtocol::OpenaiResponses => to_openai_responses(req)?,
        NativeProtocol::AnthropicMessages => to_anthropic_messages(req)?,
        NativeProtocol::GoogleGemini | NativeProtocol::GoogleVertexAi => to_google_gemini(protocol, req)?,
        NativeProtocol::AwsBedrock => to_aws_bedrock(req)?,
        NativeProtocol::Ollama => to_ollama_chat(req)?,
        NativeProtocol::Cohere => to_cohere_chat(req)?,
    };
    if let Some(obj) = payload.as_object_mut() {
        for (k, v) in &req.provider_extensions {
            obj.insert(k.clone(), v.clone());
        }
    }
    Ok(payload)
}

pub fn normalize_response(protocol: NativeProtocol, raw: &Value) -> Result<LmpResponse, LmpError> {
    match protocol {
        NativeProtocol::OpenaiChatCompletions | NativeProtocol::AzureOpenai => from_openai_chat(raw),
        NativeProtocol::OpenaiResponses => from_openai_responses(raw),
        NativeProtocol::AnthropicMessages => from_anthropic_messages(raw),
        NativeProtocol::GoogleGemini | NativeProtocol::GoogleVertexAi => from_google_gemini(raw),
        NativeProtocol::AwsBedrock => from_aws_bedrock(raw),
        NativeProtocol::Ollama => from_ollama_chat(raw),
        NativeProtocol::Cohere => from_cohere_chat(raw),
    }
}

pub fn normalize_stream_events(
    protocol: NativeProtocol,
    events: &[SseEvent],
) -> Result<Vec<LmpStreamEvent>, LmpError> {
    let mut normalizer = StreamNormalizer::new(protocol);
    let mut out = Vec::new();
    for event in events {
        normalizer.push(event, &mut out)?;
    }
    normalizer.finish(&mut out);
    Ok(out)
}

/// Stateful, protocol-aware streaming normalizer.
///
/// Streams are inherently stateful: tool arguments arrive as JSON fragments,
/// content-block indexes identify tool calls announced by an earlier frame,
/// and signatures/usage may trail the payload they describe. Keeping that
/// bookkeeping here is what lets callers normalize one event at a time instead
/// of buffering the whole upstream response.
pub struct StreamNormalizer {
    protocol: NativeProtocol,
    started: bool,
    ended: bool,
    usage: Usage,
    /// Anthropic content block index -> (tool call id, tool name).
    anthropic_tools: HashMap<usize, (Option<String>, Option<String>)>,
    /// Last finish reason observed, applied to the final message_end event.
    finish_reason: Option<String>,
    /// Gemini/Vertex send one part per frame, so the part offset is always 0;
    /// a monotonic counter keeps parallel tool calls distinguishable.
    gemini_tool_calls: usize,
}

impl StreamNormalizer {
    pub fn new(protocol: NativeProtocol) -> Self {
        Self {
            protocol,
            started: false,
            ended: false,
            usage: Usage::default(),
            anthropic_tools: HashMap::new(),
            finish_reason: None,
            gemini_tool_calls: 0,
        }
    }

    /// True once a terminal `message_end` has been produced.
    pub fn is_finished(&self) -> bool {
        self.ended
    }

    /// Normalizes a single SSE event into zero or more LMP stream events.
    pub fn push(
        &mut self,
        event: &SseEvent,
        out: &mut Vec<LmpStreamEvent>,
    ) -> Result<(), LmpError> {
        let data = event.data.trim();
        if data.is_empty() || data == "[DONE]" {
            return Ok(());
        }
        let value: Value = serde_json::from_str(data).map_err(|e| LmpError::Decode(e.to_string()))?;
        match self.protocol {
            NativeProtocol::OpenaiChatCompletions | NativeProtocol::AzureOpenai => {
                self.push_openai_chat(&value, out)
            }
            NativeProtocol::OpenaiResponses => self.push_openai_responses(event, &value, out),
            NativeProtocol::AnthropicMessages => self.push_anthropic(event, &value, out),
            NativeProtocol::GoogleGemini | NativeProtocol::GoogleVertexAi => {
                self.push_gemini(&value, out)
            }
            NativeProtocol::AwsBedrock => self.push_bedrock(&value, out),
            NativeProtocol::Ollama => self.push_ollama(&value, out),
            NativeProtocol::Cohere => self.push_cohere(event, &value, out),
        }
        Ok(())
    }

    /// Flushes state once the upstream body ends: emits `message_end` for
    /// protocols that never sent an explicit terminal frame.
    pub fn finish(&mut self, out: &mut Vec<LmpStreamEvent>) {
        if self.ended {
            return;
        }
        self.ended = true;
        out.push(LmpStreamEvent::MessageEnd {
            finish_reason: self.finish_reason.clone(),
        });
    }

    fn mark_started(&mut self, id: Option<String>, out: &mut Vec<LmpStreamEvent>) {
        if !self.started {
            self.started = true;
            out.push(LmpStreamEvent::MessageStart { id });
        }
    }

    fn mark_ended(&mut self, finish_reason: Option<String>, out: &mut Vec<LmpStreamEvent>) {
        if finish_reason.is_some() {
            self.finish_reason = finish_reason.clone();
        }
        if !self.ended {
            self.ended = true;
            out.push(LmpStreamEvent::MessageEnd { finish_reason });
        }
    }

    fn emit_usage(&mut self, usage: Usage, out: &mut Vec<LmpStreamEvent>) {
        let changed = self.usage != usage;
        self.usage = usage.clone();
        if changed {
            out.push(LmpStreamEvent::Usage { usage });
        }
    }

    fn push_openai_chat(&mut self, value: &Value, out: &mut Vec<LmpStreamEvent>) {
        if let Some(choice) = value["choices"].get(0) {
            if let Some(delta) = choice["delta"].as_object() {
                if let Some(reasoning) = delta.get("reasoning_content").and_then(Value::as_str) {
                    if !reasoning.is_empty() {
                        out.push(LmpStreamEvent::ThinkingDelta {
                            delta: reasoning.to_string(),
                            signature: None,
                        });
                    }
                }
                if let Some(text) = delta.get("content").and_then(Value::as_str) {
                    if !text.is_empty() {
                        out.push(LmpStreamEvent::TextDelta {
                            delta: text.to_string(),
                            signature: None,
                        });
                    }
                }
                if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                    for (position, call) in calls.iter().enumerate() {
                        let index = call["index"].as_u64().map(|i| i as usize).unwrap_or(position);
                        out.push(LmpStreamEvent::ToolCallDelta {
                            index,
                            id: call["id"].as_str().map(ToString::to_string),
                            name: call["function"]["name"].as_str().map(ToString::to_string),
                            arguments_delta: call["function"]["arguments"]
                                .as_str()
                                .unwrap_or_default()
                                .to_string(),
                            signature: None,
                        });
                    }
                }
            }
            if let Some(reason) = choice["finish_reason"].as_str() {
                self.mark_ended(Some(reason.to_string()), out);
            }
        }
        if let Some(usage) = value.get("usage").filter(|u| u.is_object()) {
            self.emit_usage(parse_openai_usage(usage), out);
        }
    }

    fn push_openai_responses(
        &mut self,
        event: &SseEvent,
        value: &Value,
        out: &mut Vec<LmpStreamEvent>,
    ) {
        let kind = event
            .event
            .as_deref()
            .or_else(|| value["type"].as_str())
            .unwrap_or("");
        match kind {
            "response.created" => self.mark_started(
                value["response"]["id"].as_str().map(ToString::to_string),
                out,
            ),
            "response.output_text.delta" => {
                if let Some(delta) = value["delta"].as_str() {
                    out.push(LmpStreamEvent::TextDelta {
                        delta: delta.to_string(),
                        signature: None,
                    });
                }
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                if let Some(delta) = value["delta"].as_str() {
                    out.push(LmpStreamEvent::ThinkingDelta {
                        delta: delta.to_string(),
                        signature: None,
                    });
                }
            }
            "response.output_item.added" => {
                let item = &value["item"];
                if item["type"].as_str() == Some("function_call") {
                    out.push(LmpStreamEvent::ToolCallDelta {
                        index: value["output_index"].as_u64().unwrap_or(0) as usize,
                        id: item["call_id"]
                            .as_str()
                            .or_else(|| item["id"].as_str())
                            .map(ToString::to_string),
                        name: item["name"].as_str().map(ToString::to_string),
                        arguments_delta: String::new(),
                        signature: None,
                    });
                }
            }
            "response.function_call_arguments.delta" => {
                if let Some(delta) = value["delta"].as_str() {
                    out.push(LmpStreamEvent::ToolCallDelta {
                        index: value["output_index"].as_u64().unwrap_or(0) as usize,
                        id: None,
                        name: None,
                        arguments_delta: delta.to_string(),
                        signature: None,
                    });
                }
            }
            "response.completed" => {
                if let Some(usage) = value["response"].get("usage").filter(|u| u.is_object()) {
                    let input_tokens = usage["input_tokens"].as_u64().unwrap_or(0);
                    let output_tokens = usage["output_tokens"].as_u64().unwrap_or(0);
                    self.emit_usage(
                        Usage {
                            input_tokens,
                            output_tokens,
                            total_tokens: usage["total_tokens"]
                                .as_u64()
                                .unwrap_or(input_tokens + output_tokens),
                            reasoning_tokens: usage["output_tokens_details"]["reasoning_tokens"]
                                .as_u64(),
                            cache_read_tokens: usage["input_tokens_details"]["cached_tokens"]
                                .as_u64(),
                            ..Default::default()
                        },
                        out,
                    );
                }
                self.mark_ended(
                    value["response"]["status"].as_str().map(ToString::to_string),
                    out,
                );
            }
            _ => {}
        }
    }

    fn push_anthropic(
        &mut self,
        event: &SseEvent,
        value: &Value,
        out: &mut Vec<LmpStreamEvent>,
    ) {
        let kind = event
            .event
            .as_deref()
            .or_else(|| value["type"].as_str())
            .unwrap_or("");
        match kind {
            "message_start" => {
                let message = &value["message"];
                self.mark_started(message["id"].as_str().map(ToString::to_string), out);
                if let Some(usage) = message.get("usage").filter(|u| u.is_object()) {
                    self.emit_usage(parse_anthropic_usage(usage, &self.usage), out);
                }
            }
            "content_block_start" => {
                let index = value["index"].as_u64().unwrap_or(0) as usize;
                let block = &value["content_block"];
                match block["type"].as_str() {
                    Some("tool_use") => {
                        let id = block["id"].as_str().map(ToString::to_string);
                        let name = block["name"].as_str().map(ToString::to_string);
                        self.anthropic_tools.insert(index, (id.clone(), name.clone()));
                        out.push(LmpStreamEvent::ToolCallDelta {
                            index,
                            id,
                            name,
                            arguments_delta: String::new(),
                            signature: None,
                        });
                    }
                    Some("thinking") => {
                        if let Some(signature) = block["signature"].as_str() {
                            if !signature.is_empty() {
                                out.push(LmpStreamEvent::ThinkingDelta {
                                    delta: String::new(),
                                    signature: Some(signature.to_string()),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
            "content_block_delta" => {
                let index = value["index"].as_u64().unwrap_or(0) as usize;
                let delta = &value["delta"];
                match delta["type"].as_str() {
                    Some("thinking_delta") => {
                        if let Some(text) = delta["thinking"].as_str() {
                            out.push(LmpStreamEvent::ThinkingDelta {
                                delta: text.to_string(),
                                signature: None,
                            });
                        }
                    }
                    Some("text_delta") => {
                        if let Some(text) = delta["text"].as_str() {
                            out.push(LmpStreamEvent::TextDelta {
                                delta: text.to_string(),
                                signature: None,
                            });
                        }
                    }
                    Some("input_json_delta") => {
                        if let Some(partial) = delta["partial_json"].as_str() {
                            let (id, name) = self
                                .anthropic_tools
                                .get(&index)
                                .cloned()
                                .unwrap_or((None, None));
                            out.push(LmpStreamEvent::ToolCallDelta {
                                index,
                                id,
                                name,
                                arguments_delta: partial.to_string(),
                                signature: None,
                            });
                        }
                    }
                    Some("signature_delta") => {
                        if let Some(signature) = delta["signature"].as_str() {
                            if !signature.is_empty() {
                                out.push(LmpStreamEvent::ThinkingDelta {
                                    delta: String::new(),
                                    signature: Some(signature.to_string()),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(usage) = value.get("usage").filter(|u| u.is_object()) {
                    let parsed = parse_anthropic_usage(usage, &self.usage);
                    self.emit_usage(parsed, out);
                }
                if let Some(reason) = value["delta"]["stop_reason"].as_str() {
                    self.finish_reason = Some(reason.to_string());
                }
            }
            "message_stop" => self.mark_ended(self.finish_reason.clone(), out),
            _ => {}
        }
    }

    fn push_gemini(&mut self, value: &Value, out: &mut Vec<LmpStreamEvent>) {
        if let Some(candidate) = value["candidates"].get(0) {
            if let Some(parts) = candidate["content"]["parts"].as_array() {
                for part in parts.iter() {
                    let signature = part["thoughtSignature"].as_str().map(ToString::to_string);
                    if let Some(text) = part["text"].as_str() {
                        if part["thought"].as_bool() == Some(true) {
                            out.push(LmpStreamEvent::ThinkingDelta {
                                delta: text.to_string(),
                                signature,
                            });
                        } else {
                            out.push(LmpStreamEvent::TextDelta {
                                delta: text.to_string(),
                                signature,
                            });
                        }
                    } else if let Some(call) = part.get("functionCall") {
                        let index = self.gemini_tool_calls;
                        self.gemini_tool_calls += 1;
                        out.push(LmpStreamEvent::ToolCallDelta {
                            index,
                            id: call["id"].as_str().map(ToString::to_string),
                            name: call["name"].as_str().map(ToString::to_string),
                            arguments_delta: if call["args"].is_null() {
                                String::new()
                            } else {
                                call["args"].to_string()
                            },
                            signature,
                        });
                    }
                }
            }
            if let Some(reason) = candidate["finishReason"].as_str() {
                self.finish_reason = Some(reason.to_string());
            }
        }
        if let Some(usage) = value.get("usageMetadata").filter(|u| u.is_object()) {
            let input_tokens = usage["promptTokenCount"].as_u64().unwrap_or(0);
            let output_tokens = usage["candidatesTokenCount"].as_u64().unwrap_or(0);
            self.emit_usage(
                Usage {
                    input_tokens,
                    output_tokens,
                    total_tokens: usage["totalTokenCount"]
                        .as_u64()
                        .unwrap_or(input_tokens + output_tokens),
                    reasoning_tokens: usage["thoughtsTokenCount"].as_u64(),
                    cache_read_tokens: usage["cachedContentTokenCount"].as_u64(),
                    ..Default::default()
                },
                out,
            );
        }
    }

    fn push_bedrock(&mut self, value: &Value, out: &mut Vec<LmpStreamEvent>) {
        if let Some(delta) = value["contentBlockDelta"]["delta"].as_object() {
            let index = value["contentBlockDelta"]["contentBlockIndex"]
                .as_u64()
                .unwrap_or(0) as usize;
            if let Some(text) = delta.get("text").and_then(Value::as_str) {
                out.push(LmpStreamEvent::TextDelta {
                    delta: text.to_string(),
                    signature: None,
                });
            }
            if let Some(reasoning) = delta.get("reasoningContent") {
                if let Some(text) = reasoning["text"].as_str() {
                    out.push(LmpStreamEvent::ThinkingDelta {
                        delta: text.to_string(),
                        signature: None,
                    });
                }
                if let Some(signature) = reasoning["signature"].as_str() {
                    out.push(LmpStreamEvent::ThinkingDelta {
                        delta: String::new(),
                        signature: Some(signature.to_string()),
                    });
                }
            }
            if let Some(input) = delta.get("toolUse").and_then(|v| v["input"].as_str()) {
                out.push(LmpStreamEvent::ToolCallDelta {
                    index,
                    id: None,
                    name: None,
                    arguments_delta: input.to_string(),
                    signature: None,
                });
            }
        } else if let Some(start) = value["contentBlockStart"]["start"]["toolUse"].as_object() {
            let index = value["contentBlockStart"]["contentBlockIndex"]
                .as_u64()
                .unwrap_or(0) as usize;
            out.push(LmpStreamEvent::ToolCallDelta {
                index,
                id: start
                    .get("toolUseId")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                name: start
                    .get("name")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                arguments_delta: String::new(),
                signature: None,
            });
        } else if let Some(reason) = value["messageStop"]["stopReason"].as_str() {
            self.mark_ended(Some(reason.to_string()), out);
        } else if let Some(usage) = value["metadata"].get("usage").filter(|v| v.is_object()) {
            let input_tokens = usage["inputTokens"].as_u64().unwrap_or(0);
            let output_tokens = usage["outputTokens"].as_u64().unwrap_or(0);
            self.emit_usage(
                Usage {
                    input_tokens,
                    output_tokens,
                    total_tokens: usage["totalTokens"]
                        .as_u64()
                        .unwrap_or(input_tokens + output_tokens),
                    cache_read_tokens: usage["cacheReadInputTokens"].as_u64(),
                    cache_write_tokens: usage["cacheWriteInputTokens"].as_u64(),
                    ..Default::default()
                },
                out,
            );
        }
    }

    fn push_ollama(&mut self, value: &Value, out: &mut Vec<LmpStreamEvent>) {
        if let Some(thinking) = value["message"]["thinking"].as_str() {
            if !thinking.is_empty() {
                out.push(LmpStreamEvent::ThinkingDelta {
                    delta: thinking.to_string(),
                    signature: None,
                });
            }
        }
        if let Some(text) = value["message"]["content"].as_str() {
            if !text.is_empty() {
                out.push(LmpStreamEvent::TextDelta {
                    delta: text.to_string(),
                    signature: None,
                });
            }
        }
        if let Some(calls) = value["message"]["tool_calls"].as_array() {
            for (position, call) in calls.iter().enumerate() {
                let function = &call["function"];
                let name = function["name"].as_str().unwrap_or("");
                let index = function["index"].as_u64().unwrap_or(position as u64) as usize;
                out.push(LmpStreamEvent::ToolCallDelta {
                    index,
                    id: Some(format!("ollama-call:{index}:{name}")),
                    name: Some(name.to_string()),
                    arguments_delta: function["arguments"].to_string(),
                    signature: None,
                });
            }
        }
        if value["done"].as_bool() == Some(true) {
            let input_tokens = value["prompt_eval_count"].as_u64().unwrap_or(0);
            let output_tokens = value["eval_count"].as_u64().unwrap_or(0);
            self.emit_usage(
                Usage {
                    input_tokens,
                    output_tokens,
                    total_tokens: input_tokens + output_tokens,
                    ..Default::default()
                },
                out,
            );
            self.mark_ended(
                value["done_reason"].as_str().map(ToString::to_string),
                out,
            );
        }
    }

    fn push_cohere(
        &mut self,
        event: &SseEvent,
        value: &Value,
        out: &mut Vec<LmpStreamEvent>,
    ) {
        let kind = event
            .event
            .as_deref()
            .or_else(|| value["type"].as_str())
            .unwrap_or("");
        match kind {
            "message-start" => {
                self.mark_started(value["id"].as_str().map(ToString::to_string), out);
            }
            "tool-call-start" => {
                // Cohere announces each call (id + name) then streams its
                // arguments in fragments; LMP keeps the same split.
                let index = value["index"].as_u64().unwrap_or(0) as usize;
                let call = &value["delta"]["message"]["tool_calls"];
                let call = match call.as_array().and_then(|a| a.first()) {
                    Some(call) => call,
                    None => call,
                };
                out.push(LmpStreamEvent::ToolCallDelta {
                    index,
                    id: call["id"].as_str().map(ToString::to_string),
                    name: call["function"]["name"].as_str().map(ToString::to_string),
                    arguments_delta: String::new(),
                    signature: None,
                });
            }
            "tool-call-delta" => {
                let index = value["index"].as_u64().unwrap_or(0) as usize;
                if let Some(delta) =
                    value["delta"]["message"]["tool_calls"]["function"]["arguments"].as_str()
                {
                    out.push(LmpStreamEvent::ToolCallDelta {
                        index,
                        id: None,
                        name: None,
                        arguments_delta: delta.to_string(),
                        signature: None,
                    });
                }
            }
            // `tool-plan-delta` is Cohere's natural-language plan, not model
            // reasoning, so it has no LMP event; `tool-call-end` closes a call
            // whose deltas were already forwarded.
            "content-delta" => {
                if let Some(text) = value["delta"]["message"]["content"]["text"].as_str() {
                    out.push(LmpStreamEvent::TextDelta {
                        delta: text.to_string(),
                        signature: None,
                    });
                }
                if let Some(thinking) = value["delta"]["message"]["content"]["thinking"].as_str() {
                    out.push(LmpStreamEvent::ThinkingDelta {
                        delta: thinking.to_string(),
                        signature: None,
                    });
                }
            }
            "message-end" => {
                if let Some(usage) = value["delta"]["usage"].get("tokens") {
                    let input_tokens = usage["input_tokens"].as_u64().unwrap_or(0);
                    let output_tokens = usage["output_tokens"].as_u64().unwrap_or(0);
                    self.emit_usage(
                        Usage {
                            input_tokens,
                            output_tokens,
                            total_tokens: input_tokens + output_tokens,
                            ..Default::default()
                        },
                        out,
                    );
                }
                self.mark_ended(
                    value["delta"]["finish_reason"]
                        .as_str()
                        .map(ToString::to_string),
                    out,
                );
            }
            _ => {}
        }
    }
}

/// Anthropic reports cache counters separately from input tokens; LMP keeps
/// both on one usage record. Stream deltas only carry the new counters, so the
/// previous totals are merged in.
fn parse_anthropic_usage(usage: &Value, previous: &Usage) -> Usage {
    let input_tokens = usage["input_tokens"]
        .as_u64()
        .unwrap_or(previous.input_tokens);
    let output_tokens = usage["output_tokens"]
        .as_u64()
        .unwrap_or(previous.output_tokens);
    Usage {
        input_tokens,
        output_tokens,
        total_tokens: usage["total_tokens"]
            .as_u64()
            .unwrap_or(input_tokens + output_tokens),
        cache_read_tokens: usage["cache_read_input_tokens"]
            .as_u64()
            .or(previous.cache_read_tokens),
        cache_write_tokens: usage["cache_creation_input_tokens"]
            .as_u64()
            .or(previous.cache_write_tokens),
        ..Default::default()
    }
}

fn to_openai_chat(req: &LmpRequest) -> Result<Value, LmpError> {
    if let Some(ref t) = req.thinking {
        if t.budget_tokens.is_some() {
            return Err(LmpError::UnsupportedFeature {
                protocol: "openai_chat_completions".into(),
                feature: "budget_tokens".into(),
            });
        }
    }
    let mut messages = Vec::new();
    for msg in &req.messages {
        let role = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        let mut text_parts = Vec::new();
        let mut multi_parts = Vec::new();
        let mut has_non_text = false;
        let mut tool_calls = Vec::new();
        // OpenAI Chat carries one tool result per message. Every ToolResult
        // block therefore becomes its own role=tool message; folding them into
        // the surrounding message dropped all but the last call id.
        let mut tool_results = Vec::new();

        for block in &msg.content {
            match block {
                ContentBlock::Text { text, .. } => {
                    text_parts.push(text.clone());
                    multi_parts.push(json!({"type": "text", "text": text}));
                }
                ContentBlock::ImageUrl { url } => {
                    has_non_text = true;
                    multi_parts.push(json!({"type": "image_url", "image_url": {"url": url}}));
                }
                ContentBlock::ImageBase64 { media_type, data } => {
                    has_non_text = true;
                    multi_parts.push(json!({
                        "type": "image_url",
                        "image_url": {"url": format!("data:{media_type};base64,{data}")}
                    }));
                }
                ContentBlock::ToolCall { id, name, arguments, .. } => {
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {"name": name, "arguments": arguments.to_string()}
                    }));
                }
                ContentBlock::ToolResult {
                    tool_call_id: id,
                    content,
                    ..
                } => {
                    tool_results.push((id.clone(), content.clone()));
                }
                ContentBlock::Audio { .. } => {
                    return Err(LmpError::UnsupportedFeature {
                        protocol: "openai_chat_completions".into(),
                        feature: "audio_block".into(),
                    });
                }
                ContentBlock::Thinking { .. } => {}
            }
        }
        if !text_parts.is_empty() || has_non_text || !tool_calls.is_empty() {
            let mut m = Map::new();
            m.insert("role".into(), json!(role));
            if has_non_text {
                m.insert("content".into(), Value::Array(multi_parts));
            } else {
                m.insert("content".into(), json!(text_parts.join("")));
            }
            if !tool_calls.is_empty() {
                m.insert("tool_calls".into(), Value::Array(tool_calls));
            }
            messages.push(Value::Object(m));
        }
        for (id, content) in tool_results {
            messages.push(json!({"role": "tool", "tool_call_id": id, "content": content}));
        }
    }

    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert("messages".into(), Value::Array(messages));
    if let Some(max) = req.max_tokens {
        body.insert("max_tokens".into(), json!(max));
    }
    if let Some(temp) = req.temperature {
        body.insert("temperature".into(), json!(temp));
    }
    if let Some(effort) = req.thinking.as_ref().and_then(|t| t.effort.as_ref()) {
        body.insert("reasoning_effort".into(), json!(effort));
    }
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            })
            .collect();
        body.insert("tools".into(), Value::Array(tools));
    }
    if let Some(ref tc) = req.tool_choice {
        body.insert(
            "tool_choice".into(),
            match tc {
                ToolChoice::Auto => json!("auto"),
                ToolChoice::None => json!("none"),
                ToolChoice::Required => json!("required"),
                ToolChoice::Tool(name) => json!({"type": "function", "function": {"name": name}}),
            },
        );
    }
    if let Some(ref rf) = req.response_format {
        body.insert(
            "response_format".into(),
            match rf {
                ResponseFormat::Text => json!({"type": "text"}),
                ResponseFormat::JsonObject => json!({"type": "json_object"}),
                ResponseFormat::JsonSchema {
                    name,
                    schema,
                    strict,
                } => json!({
                    "type": "json_schema",
                    "json_schema": {
                        "name": name,
                        "schema": schema,
                        "strict": strict.unwrap_or(true)
                    }
                }),
            },
        );
    }
    Ok(Value::Object(body))
}

fn to_openai_responses(req: &LmpRequest) -> Result<Value, LmpError> {
    if let Some(ref t) = req.thinking {
        if t.budget_tokens.is_some() {
            return Err(LmpError::UnsupportedFeature {
                protocol: "openai_responses".into(),
                feature: "budget_tokens".into(),
            });
        }
    }
    let mut input = Vec::new();
    for msg in &req.messages {
        let role = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "user",
        };
        // Responses replays a turn as separate input items instead of one
        // message. Text/image parts stay inside a role item, while reasoning and
        // function_call items must be emitted in their original order or the
        // model loses the reasoning state and the tool call it produced.
        let mut parts = Vec::new();
        for b in &msg.content {
            match b {
                ContentBlock::Text { text, .. } => {
                    parts.push(json!({"type": "input_text", "text": text}));
                }
                ContentBlock::ImageUrl { url } => {
                    parts.push(json!({"type": "input_image", "image_url": url}));
                }
                ContentBlock::ImageBase64 { media_type, data } => {
                    parts.push(json!({
                        "type": "input_image",
                        "image_url": format!("data:{media_type};base64,{data}")
                    }));
                }
                ContentBlock::Thinking {
                    thinking,
                    signature,
                } => {
                    if !parts.is_empty() {
                        input.push(json!({"role": role, "content": parts}));
                        parts = Vec::new();
                    }
                    // `signature` carries the opaque `encrypted_content` token so a
                    // stateless replay (store=false) keeps working reasoning state.
                    let mut item = json!({
                        "type": "reasoning",
                        "summary": [{"type": "summary_text", "text": thinking}]
                    });
                    if let Some(sig) = signature {
                        item["encrypted_content"] = json!(sig);
                    }
                    input.push(item);
                }
                ContentBlock::ToolCall {
                    id,
                    name,
                    arguments,
                    ..
                } => {
                    if !parts.is_empty() {
                        input.push(json!({"role": role, "content": parts}));
                        parts = Vec::new();
                    }
                    input.push(json!({
                        "type": "function_call",
                        "call_id": id,
                        "name": name,
                        "arguments": arguments.to_string()
                    }));
                }
                ContentBlock::ToolResult {
                    tool_call_id,
                    content,
                    ..
                } => {
                    input.push(json!({
                        "type": "function_call_output",
                        "call_id": tool_call_id,
                        "output": content
                    }));
                }
                ContentBlock::Audio { .. } => {
                    return Err(LmpError::UnsupportedFeature {
                        protocol: "openai_responses".into(),
                        feature: "audio_block".into(),
                    });
                }
            }
        }
        if !parts.is_empty() {
            input.push(json!({"role": role, "content": parts}));
        }
    }
    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert("input".into(), Value::Array(input));
    if let Some(max) = req.max_tokens {
        body.insert("max_output_tokens".into(), json!(max));
    }
    if let Some(effort) = req.thinking.as_ref().and_then(|t| t.effort.as_ref()) {
        body.insert("reasoning".into(), json!({"effort": effort}));
    }
    if let Some(ref rf) = req.response_format {
        match rf {
            ResponseFormat::Text => {}
            ResponseFormat::JsonObject => {
                body.insert("text".into(), json!({"format": {"type": "json_object"}}));
            }
            ResponseFormat::JsonSchema {
                name,
                schema,
                strict,
            } => {
                body.insert(
                    "text".into(),
                    json!({
                        "format": {
                            "type": "json_schema",
                            "name": name,
                            "schema": schema,
                            "strict": strict.unwrap_or(true)
                        }
                    }),
                );
            }
        }
    }
    Ok(Value::Object(body))
}

fn to_anthropic_messages(req: &LmpRequest) -> Result<Value, LmpError> {
    if let Some(ResponseFormat::JsonSchema { .. } | ResponseFormat::JsonObject) =
        req.response_format
    {
        return Err(LmpError::UnsupportedFeature {
            protocol: "anthropic_messages".into(),
            feature: "json_schema".into(),
        });
    }
    // Anthropic rejects forced tool use (`any` / `tool`) together with manual
    // extended thinking. Report it here instead of forwarding a request the
    // provider answers with an opaque 400.
    if req.thinking.as_ref().and_then(|t| t.budget_tokens).is_some()
        && matches!(
            req.tool_choice,
            Some(ToolChoice::Required | ToolChoice::Tool(_))
        )
    {
        return Err(LmpError::UnsupportedFeature {
            protocol: "anthropic_messages".into(),
            feature: "forced_tool_choice_with_thinking".into(),
        });
    }
    let mut system_parts = Vec::new();
    let mut messages: Vec<Value> = Vec::new();
    for msg in &req.messages {
        if msg.role == Role::System {
            for b in &msg.content {
                if let ContentBlock::Text { text, .. } = b {
                    system_parts.push(text.clone());
                }
            }
            continue;
        }
        let role = match msg.role {
            Role::Assistant => "assistant",
            _ => "user",
        };
        let mut blocks = Vec::new();
        for b in &msg.content {
            match b {
                ContentBlock::Text { text, .. } => blocks.push(json!({"type": "text", "text": text})),
                ContentBlock::ImageBase64 { media_type, data } => blocks.push(json!({
                    "type": "image",
                    "source": {"type": "base64", "media_type": media_type, "data": data}
                })),
                ContentBlock::ImageUrl { url } => blocks.push(json!({
                    "type": "image",
                    "source": {"type": "url", "url": url}
                })),
                ContentBlock::Thinking {
                    thinking,
                    signature,
                } => {
                    let mut obj = Map::new();
                    obj.insert("type".into(), json!("thinking"));
                    obj.insert("thinking".into(), json!(thinking));
                    if let Some(sig) = signature {
                        obj.insert("signature".into(), json!(sig));
                    }
                    blocks.push(Value::Object(obj));
                }
                ContentBlock::ToolCall {
                    id,
                    name,
                    arguments,
                    ..
                } => {
                    blocks.push(json!({"type": "tool_use", "id": id, "name": name, "input": arguments}));
                }
                ContentBlock::ToolResult {
                    tool_call_id,
                    content,
                    is_error,
                } => {
                    blocks.push(json!({
                        "type": "tool_result",
                        "tool_use_id": tool_call_id,
                        "content": content,
                        "is_error": is_error
                    }));
                }
                ContentBlock::Audio { .. } => {
                    return Err(LmpError::UnsupportedFeature {
                        protocol: "anthropic_messages".into(),
                        feature: "audio_block".into(),
                    });
                }
            }
        }
        // Anthropic expects every tool_result for one assistant turn inside a
        // single user message; consecutive same-role turns are merged here so a
        // caller that emits one LMP tool message per result still gets one.
        if let Some(last) = messages.last_mut() {
            if last["role"] == json!(role) {
                if let Some(existing) = last["content"].as_array_mut() {
                    existing.extend(blocks.into_iter());
                    continue;
                }
            }
        }
        messages.push(json!({"role": role, "content": blocks}));
    }

    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert("max_tokens".into(), json!(req.max_tokens.unwrap_or(4096)));
    body.insert("messages".into(), Value::Array(messages));
    if !system_parts.is_empty() {
        body.insert("system".into(), json!(system_parts.join("\n")));
    }
    if let Some(temp) = req.temperature {
        body.insert("temperature".into(), json!(temp));
    }
    if let Some(budget) = req.thinking.as_ref().and_then(|t| t.budget_tokens) {
        body.insert(
            "thinking".into(),
            json!({"type": "enabled", "budget_tokens": budget}),
        );
    }
    // Anthropic steers thinking depth through `output_config.effort` on models
    // that support it; there is no per-request budget equivalent for `effort`.
    if let Some(effort) = req.thinking.as_ref().and_then(|t| t.effort.as_deref()) {
        body.insert("output_config".into(), json!({"effort": effort}));
    }
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters
                })
            })
            .collect();
        body.insert("tools".into(), Value::Array(tools));
    }
    if let Some(ref tc) = req.tool_choice {
        body.insert(
            "tool_choice".into(),
            match tc {
                ToolChoice::Auto => json!({"type": "auto"}),
                ToolChoice::None => json!({"type": "none"}),
                ToolChoice::Required => json!({"type": "any"}),
                ToolChoice::Tool(name) => json!({"type": "tool", "name": name}),
            },
        );
    }
    Ok(Value::Object(body))
}

fn to_google_gemini(protocol: NativeProtocol, req: &LmpRequest) -> Result<Value, LmpError> {
    let unsupported = |feature: &str| LmpError::UnsupportedFeature {
        protocol: protocol.as_str().into(),
        feature: feature.into(),
    };
    let mut system_parts = Vec::new();
    let mut contents = Vec::new();
    let mut calls = std::collections::HashMap::new();
    let mut previous_role = None;
    for msg in &req.messages {
        if msg.role == Role::System {
            for b in &msg.content {
                match b {
                    ContentBlock::Text { text, signature: None } => system_parts.push(json!({"text": text})),
                    _ => return Err(unsupported("system_content")),
                }
            }
            continue;
        }
        let role = if msg.role == Role::Assistant {
            calls.clear();
            "model"
        } else {
            "user"
        };
        let mut parts = Vec::new();
        for b in &msg.content {
            let (mut part, signature) = match b {
                ContentBlock::Text { text, signature } => (json!({"text": text}), signature.as_deref()),
                ContentBlock::ImageBase64 { media_type, data } |
                ContentBlock::Audio { media_type, data } => (json!({"inlineData": {"mimeType": media_type, "data": data}}), None),
                ContentBlock::ImageUrl { .. } => return Err(unsupported("image_url")),
                ContentBlock::Thinking { thinking, signature } => (json!({"thought": true, "text": thinking}), signature.as_deref()),
                ContentBlock::ToolCall { id, name, arguments, signature } => {
                    let wire_id = !id.is_empty() && id != name && !id.starts_with("gemini-part:");
                    if calls.insert(id, (name, wire_id)).is_some() {
                        return Err(unsupported("duplicate_tool_call_id"));
                    }
                    let mut call = json!({"name": name, "args": arguments});
                    if wire_id {
                        call["id"] = json!(id);
                    }
                    (json!({"functionCall": call}), signature.as_deref())
                }
                ContentBlock::ToolResult { tool_call_id, content, is_error } => {
                    let (name, wire_id) = calls.get(tool_call_id).ok_or_else(|| unsupported("unmatched_tool_result"))?;
                    let key = if *is_error { "error" } else { "content" };
                    let mut response = json!({"name": name, "response": {key: content}});
                    if *wire_id {
                        response["id"] = json!(tool_call_id);
                    }
                    (json!({"functionResponse": response}), None)
                }
            };
            if let Some(sig) = signature {
                part["thoughtSignature"] = json!(sig);
            }
            parts.push(part);
        }
        if msg.role == Role::Tool && previous_role == Some(Role::Tool) {
            if let Some(Value::Array(previous)) = contents.last_mut().and_then(|v: &mut Value| v.get_mut("parts")) {
                previous.extend(parts);
            }
        } else {
            contents.push(json!({"role": role, "parts": parts}));
        }
        previous_role = Some(msg.role.clone());
    }

    let mut body = Map::new();
    body.insert("contents".into(), Value::Array(contents));
    if !system_parts.is_empty() {
        body.insert("systemInstruction".into(), json!({"parts": system_parts}));
    }
    let mut gen_cfg = Map::new();
    if let Some(max) = req.max_tokens {
        gen_cfg.insert("maxOutputTokens".into(), json!(max));
    }
    if let Some(temp) = req.temperature {
        gen_cfg.insert("temperature".into(), json!(temp));
    }
    if let Some(ref rf) = req.response_format {
        match rf {
            ResponseFormat::Text => {}
            ResponseFormat::JsonObject => {
                gen_cfg.insert("responseMimeType".into(), json!("application/json"));
            }
            ResponseFormat::JsonSchema { schema, .. } => {
                gen_cfg.insert("responseMimeType".into(), json!("application/json"));
                gen_cfg.insert("responseSchema".into(), schema.clone());
            }
        }
    }
    if let Some(ref t) = req.thinking {
        match (t.budget_tokens, t.effort.as_deref()) {
            (Some(_), Some(_)) => return Err(unsupported("thinking_budget_and_effort")),
            (Some(budget), None) => {
                gen_cfg.insert("thinkingConfig".into(), json!({"thinkingBudget": budget, "includeThoughts": true}));
            }
            (None, Some(effort @ ("minimal" | "low" | "medium" | "high"))) => {
                gen_cfg.insert("thinkingConfig".into(), json!({"thinkingLevel": effort.to_ascii_uppercase(), "includeThoughts": true}));
            }
            (None, Some(_)) => return Err(unsupported("thinking_effort")),
            (None, None) => {}
        }
    }
    if !gen_cfg.is_empty() {
        body.insert("generationConfig".into(), Value::Object(gen_cfg));
    }
    if !req.tools.is_empty() {
        let decls: Vec<Value> = req
            .tools
            .iter()
            .map(|t| json!({"name": t.name, "description": t.description, "parameters": t.parameters}))
            .collect();
        body.insert("tools".into(), json!([{"functionDeclarations": decls}]));
    }
    if let Some(ref tc) = req.tool_choice {
        if req.tools.is_empty() {
            return Err(unsupported("tool_choice_without_tools"));
        }
        // Gemini's toolConfig is the only way to force a call; `any` can be
        // narrowed to a single function via allowedFunctionNames.
        let mut config = Map::new();
        match tc {
            ToolChoice::Auto => {
                config.insert("mode".into(), json!("AUTO"));
            }
            ToolChoice::None => {
                config.insert("mode".into(), json!("NONE"));
            }
            ToolChoice::Required => {
                config.insert("mode".into(), json!("ANY"));
            }
            ToolChoice::Tool(name) => {
                config.insert("mode".into(), json!("ANY"));
                config.insert("allowedFunctionNames".into(), json!([name]));
            }
        }
        body.insert(
            "toolConfig".into(),
            json!({"functionCallingConfig": Value::Object(config)}),
        );
    }
    Ok(Value::Object(body))
}

fn from_openai_chat(raw: &Value) -> Result<LmpResponse, LmpError> {
    let choice = raw["choices"]
        .get(0)
        .ok_or_else(|| LmpError::Decode("missing choices[0]".into()))?;
    let msg = &choice["message"];
    let mut content = Vec::new();
    if let Some(rc) = msg["reasoning_content"].as_str() {
        if !rc.is_empty() {
            content.push(ContentBlock::Thinking {
                thinking: rc.to_string(),
                signature: None,
            });
        }
    }
    if let Some(txt) = msg["content"].as_str() {
        if !txt.is_empty() {
            content.push(ContentBlock::Text {
                text: txt.to_string(),
                signature: None,
            });
        }
    }
    if let Some(tcs) = msg["tool_calls"].as_array() {
        for tc in tcs {
            let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
            let args = serde_json::from_str(args_str).unwrap_or_else(|_| json!({}));
            content.push(ContentBlock::ToolCall {
                id: tc["id"].as_str().unwrap_or("").to_string(),
                name: tc["function"]["name"].as_str().unwrap_or("").to_string(),
                arguments: args,
                signature: None,
            });
        }
    }
    Ok(LmpResponse {
        id: raw["id"].as_str().map(ToString::to_string),
        model: raw["model"].as_str().map(ToString::to_string),
        content,
        finish_reason: choice["finish_reason"].as_str().map(ToString::to_string),
        usage: parse_openai_usage(&raw["usage"]),
        raw: raw.clone(),
    })
}

fn from_openai_responses(raw: &Value) -> Result<LmpResponse, LmpError> {
    let mut content = Vec::new();
    if let Some(output) = raw["output"].as_array() {
        for item in output {
            match item["type"].as_str() {
                Some("reasoning") => {
                    if let Some(sum) = item["summary"].as_array() {
                        let text = sum
                            .iter()
                            .filter_map(|s| s["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        // Keep the encrypted reasoning token: it is what makes a
                        // stateless replay of this turn preserve reasoning state.
                        let signature = item["encrypted_content"]
                            .as_str()
                            .map(ToString::to_string);
                        if !text.is_empty() || signature.is_some() {
                            content.push(ContentBlock::Thinking {
                                thinking: text,
                                signature,
                            });
                        }
                    }
                }
                Some("message") => {
                    if let Some(parts) = item["content"].as_array() {
                        for p in parts {
                            if let Some(txt) = p["text"].as_str() {
                                content.push(ContentBlock::Text {
                                    text: txt.to_string(),
                                    signature: None,
                                });
                            }
                        }
                    }
                }
                Some("function_call") => {
                    let args = item["arguments"]
                        .as_str()
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or_else(|| json!({}));
                    content.push(ContentBlock::ToolCall {
                        id: item["call_id"].as_str().unwrap_or("").to_string(),
                        name: item["name"].as_str().unwrap_or("").to_string(),
                        arguments: args,
                signature: None,
                    });
                }
                _ => {}
            }
        }
    }
    let u = &raw["usage"];
    let usage = Usage {
        input_tokens: u["input_tokens"].as_u64().unwrap_or(0),
        output_tokens: u["output_tokens"].as_u64().unwrap_or(0),
        total_tokens: u["total_tokens"].as_u64().unwrap_or(0),
        reasoning_tokens: u["output_tokens_details"]["reasoning_tokens"].as_u64(),
        cache_read_tokens: u["input_tokens_details"]["cached_tokens"].as_u64(),
        cache_write_tokens: None,
    };
    Ok(LmpResponse {
        id: raw["id"].as_str().map(ToString::to_string),
        model: raw["model"].as_str().map(ToString::to_string),
        content,
        finish_reason: raw["status"].as_str().map(ToString::to_string),
        usage,
        raw: raw.clone(),
    })
}

fn from_anthropic_messages(raw: &Value) -> Result<LmpResponse, LmpError> {
    let mut content = Vec::new();
    if let Some(blocks) = raw["content"].as_array() {
        for b in blocks {
            match b["type"].as_str() {
                Some("thinking") => content.push(ContentBlock::Thinking {
                    thinking: b["thinking"].as_str().unwrap_or("").to_string(),
                    signature: b["signature"].as_str().map(ToString::to_string),
                }),
                Some("text") => content.push(ContentBlock::Text {
                    text: b["text"].as_str().unwrap_or("").to_string(),
                    signature: None,
                }),
                Some("tool_use") => content.push(ContentBlock::ToolCall {
                    id: b["id"].as_str().unwrap_or("").to_string(),
                    name: b["name"].as_str().unwrap_or("").to_string(),
                    arguments: b["input"].clone(),
                signature: None,
                }),
                _ => {}
            }
        }
    }
    let u = &raw["usage"];
    let in_tok = u["input_tokens"].as_u64().unwrap_or(0);
    let out_tok = u["output_tokens"].as_u64().unwrap_or(0);
    Ok(LmpResponse {
        id: raw["id"].as_str().map(ToString::to_string),
        model: raw["model"].as_str().map(ToString::to_string),
        content,
        finish_reason: raw["stop_reason"].as_str().map(ToString::to_string),
        usage: Usage {
            input_tokens: in_tok,
            output_tokens: out_tok,
            total_tokens: in_tok + out_tok,
            reasoning_tokens: None,
            cache_read_tokens: u["cache_read_input_tokens"].as_u64(),
            cache_write_tokens: u["cache_creation_input_tokens"].as_u64(),
        },
        raw: raw.clone(),
    })
}

fn from_google_gemini(raw: &Value) -> Result<LmpResponse, LmpError> {
    let cand = raw["candidates"]
        .get(0)
        .ok_or_else(|| LmpError::Decode("missing candidates[0]".into()))?;
    let mut content = Vec::new();
    if let Some(parts) = cand["content"]["parts"].as_array() {
        for (index, p) in parts.iter().enumerate() {
            let signature = p["thoughtSignature"].as_str().map(str::to_owned);
            if let Some(txt) = p["text"].as_str() {
                if p["thought"].as_bool() == Some(true) {
                    content.push(ContentBlock::Thinking { thinking: txt.to_owned(), signature });
                } else {
                    content.push(ContentBlock::Text { text: txt.to_owned(), signature });
                }
            } else if let Some(call) = p["functionCall"].as_object() {
                let name = call.get("name").and_then(Value::as_str)
                    .ok_or_else(|| LmpError::Decode("functionCall missing name".into()))?;
                content.push(ContentBlock::ToolCall {
                    id: call.get("id").and_then(Value::as_str).map(str::to_owned)
                        .unwrap_or_else(|| format!("gemini-part:{index}:{name}")),
                    name: name.to_owned(),
                    arguments: call.get("args").cloned().unwrap_or_else(|| json!({})),
                    signature,
                });
            } else if signature.is_some() {
                return Err(LmpError::Decode("Gemini returned a signature on an unsupported part".into()));
            }
        }
    }
    let u = &raw["usageMetadata"];
    Ok(LmpResponse {
        id: None,
        model: None,
        content,
        finish_reason: cand["finishReason"].as_str().map(ToString::to_string),
        usage: Usage {
            input_tokens: u["promptTokenCount"].as_u64().unwrap_or(0),
            output_tokens: u["candidatesTokenCount"].as_u64().unwrap_or(0),
            total_tokens: u["totalTokenCount"].as_u64().unwrap_or(0),
            reasoning_tokens: u["thoughtsTokenCount"].as_u64(),
            cache_read_tokens: u["cachedContentTokenCount"].as_u64(),
            cache_write_tokens: None,
        },
        raw: raw.clone(),
    })
}

fn parse_openai_usage(u: &Value) -> Usage {
    Usage {
        input_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
        output_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
        total_tokens: u["total_tokens"].as_u64().unwrap_or(0),
        reasoning_tokens: u["completion_tokens_details"]["reasoning_tokens"].as_u64(),
        cache_read_tokens: u["prompt_tokens_details"]["cached_tokens"].as_u64(),
        cache_write_tokens: None,
    }
}

fn to_aws_bedrock(req: &LmpRequest) -> Result<Value, LmpError> {
    let mut system = Vec::new();
    let mut messages = Vec::new();
    for msg in &req.messages {
        if msg.role == Role::System {
            for b in &msg.content {
                if let ContentBlock::Text { text, .. } = b {
                    system.push(json!({"text": text}));
                }
            }
            continue;
        }
        let role = match msg.role {
            Role::Assistant => "assistant",
            _ => "user",
        };
        let mut blocks = Vec::new();
        for b in &msg.content {
            match b {
                ContentBlock::Text { text, .. } => blocks.push(json!({"text": text})),
                ContentBlock::Thinking { thinking, signature } => {
                    let mut rc = json!({"text": thinking});
                    if let Some(sig) = signature {
                        rc["signature"] = json!(sig);
                    }
                    blocks.push(json!({"reasoningContent": {"reasoningText": rc}}));
                }
                ContentBlock::ToolCall { id, name, arguments, .. } => {
                    blocks.push(json!({"toolUse": {"toolUseId": id, "name": name, "input": arguments}}));
                }
                ContentBlock::ToolResult { tool_call_id, content, is_error } => {
                    blocks.push(json!({
                        "toolResult": {
                            "toolUseId": tool_call_id,
                            "content": [{"text": content}],
                            "status": if *is_error { "error" } else { "success" }
                        }
                    }));
                }
                ContentBlock::ImageBase64 { media_type, data } => {
                    // Bedrock Converse takes inline bytes plus a format enum, not a
                    // media type; an unknown format must fail loudly.
                    let format = match media_type.as_str() {
                        "image/png" => "png",
                        "image/jpeg" => "jpeg",
                        "image/gif" => "gif",
                        "image/webp" => "webp",
                        other => {
                            return Err(LmpError::UnsupportedFeature {
                                protocol: "aws_bedrock".into(),
                                feature: format!("image_media_type:{other}"),
                            })
                        }
                    };
                    blocks.push(json!({
                        "image": {"format": format, "source": {"bytes": data}}
                    }));
                }
                // Bedrock accepts inline bytes or an S3 location only, so an LMP
                // image URL has no lossless mapping.
                ContentBlock::ImageUrl { .. } => {
                    return Err(LmpError::UnsupportedFeature {
                        protocol: "aws_bedrock".into(),
                        feature: "image_url".into(),
                    });
                }
                ContentBlock::Audio { .. } => {
                    return Err(LmpError::UnsupportedFeature {
                        protocol: "aws_bedrock".into(),
                        feature: "audio_input".into(),
                    });
                }
            }
        }
        messages.push(json!({"role": role, "content": blocks}));
    }
    let mut body = Map::new();
    body.insert("messages".into(), Value::Array(messages));
    if !system.is_empty() {
        body.insert("system".into(), Value::Array(system));
    }
    let mut inference = Map::new();
    if let Some(max) = req.max_tokens {
        inference.insert("maxTokens".into(), json!(max));
    }
    if let Some(temp) = req.temperature {
        inference.insert("temperature".into(), json!(temp));
    }
    if !inference.is_empty() {
        body.insert("inferenceConfig".into(), Value::Object(inference));
    }
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "toolSpec": {
                        "name": t.name,
                        "description": t.description.as_deref().unwrap_or(""),
                        "inputSchema": {"json": t.parameters}
                    }
                })
            })
            .collect();
        let mut config = Map::new();
        config.insert("tools".into(), Value::Array(tools));
        if let Some(ref tc) = req.tool_choice {
            // Bedrock models decide whether they honour a forced choice; the
            // wire shape is a one-member union. `none` has no Bedrock
            // equivalent, so it must fail loudly rather than silently degrade.
            let choice = match tc {
                ToolChoice::Auto => json!({"auto": {}}),
                ToolChoice::Required => json!({"any": {}}),
                ToolChoice::Tool(name) => json!({"tool": {"name": name}}),
                ToolChoice::None => return Err(LmpError::UnsupportedFeature {
                    protocol: "aws_bedrock".into(),
                    feature: "tool_choice_none".into(),
                }),
            };
            config.insert("toolChoice".into(), choice);
        }
        body.insert("toolConfig".into(), Value::Object(config));
    }
    if let Some(ref t) = req.thinking {
        if let Some(budget) = t.budget_tokens {
            body.insert(
                "additionalModelRequestFields".into(),
                json!({"thinking": {"type": "enabled", "budget_tokens": budget}}),
            );
        }
    }
    Ok(Value::Object(body))
}

fn from_aws_bedrock(raw: &Value) -> Result<LmpResponse, LmpError> {
    let mut content = Vec::new();
    if let Some(blocks) = raw["output"]["message"]["content"].as_array() {
        for b in blocks {
            if let Some(txt) = b["text"].as_str() {
                content.push(ContentBlock::Text { text: txt.to_string(), signature: None });
            } else if let Some(rt) = b["reasoningContent"]["reasoningText"].as_object() {
                content.push(ContentBlock::Thinking {
                    thinking: rt.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
                    signature: rt.get("signature").and_then(Value::as_str).map(ToString::to_string),
                });
            } else if let Some(tu) = b["toolUse"].as_object() {
                content.push(ContentBlock::ToolCall {
                    id: tu.get("toolUseId").and_then(Value::as_str).unwrap_or("").to_string(),
                    name: tu.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                    arguments: tu.get("input").cloned().unwrap_or(Value::Null),
                signature: None,
                });
            }
        }
    }
    let u = &raw["usage"];
    Ok(LmpResponse {
        id: None,
        model: None,
        content,
        finish_reason: raw["stopReason"].as_str().map(ToString::to_string),
        usage: Usage {
            input_tokens: u["inputTokens"].as_u64().unwrap_or(0),
            output_tokens: u["outputTokens"].as_u64().unwrap_or(0),
            total_tokens: u["totalTokens"].as_u64().unwrap_or(0),
            reasoning_tokens: None,
            cache_read_tokens: u["cacheReadInputTokens"].as_u64(),
            cache_write_tokens: u["cacheWriteInputTokens"].as_u64(),
        },
        raw: raw.clone(),
    })
}

fn to_ollama_chat(req: &LmpRequest) -> Result<Value, LmpError> {
    if let Some(ResponseFormat::JsonSchema { .. }) = req.response_format {
        return Err(LmpError::UnsupportedFeature {
            protocol: "ollama".into(),
            feature: "json_schema_strict".into(),
        });
    }
    // Ollama has no tool_choice field; `auto` is the default behaviour and `none`
    // is expressed by withholding the declarations. Forcing a specific call is
    // not representable, so it must fail loudly.
    let mut declare_tools = !req.tools.is_empty();
    match req.tool_choice {
        None | Some(ToolChoice::Auto) => {}
        Some(ToolChoice::None) => declare_tools = false,
        Some(ToolChoice::Required | ToolChoice::Tool(_)) => {
            return Err(LmpError::UnsupportedFeature {
                protocol: "ollama".into(),
                feature: "tool_choice".into(),
            });
        }
    }
    // Ollama correlates a tool result with its call through the function name,
    // so remember which call produced each id while replaying the turn.
    let mut call_names = HashMap::new();
    let mut messages = Vec::new();
    for msg in &req.messages {
        let role = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        let mut text = String::new();
        let mut thinking = String::new();
        let mut images = Vec::new();
        let mut tool_calls = Vec::new();
        let mut tool_results = Vec::new();
        for b in &msg.content {
            match b {
                ContentBlock::Text { text: t, .. } => text.push_str(t),
                ContentBlock::Thinking { thinking: t, .. } => thinking.push_str(t),
                ContentBlock::ImageBase64 { data, .. } => images.push(json!(data)),
                // Ollama accepts either raw base64 or a remote URL in `images`.
                ContentBlock::ImageUrl { url } => images.push(json!(url)),
                ContentBlock::ToolCall { id, name, arguments, .. } => {
                    call_names.insert(id.clone(), name.clone());
                    // Ollama expects the arguments as a JSON object, not a string.
                    tool_calls.push(json!({
                        "type": "function",
                        "function": {
                            "index": tool_calls.len(),
                            "name": name,
                            "arguments": arguments
                        }
                    }));
                }
                ContentBlock::ToolResult { tool_call_id, content, .. } => {
                    let name = call_names.get(tool_call_id).cloned().unwrap_or_default();
                    tool_results.push((name, content.clone()));
                }
                ContentBlock::Audio { .. } => {
                    return Err(LmpError::UnsupportedFeature {
                        protocol: "ollama".into(),
                        feature: "audio_input".into(),
                    });
                }
            }
        }
        if !text.is_empty() || !thinking.is_empty() || !images.is_empty() || !tool_calls.is_empty() {
            let mut m = json!({"role": role, "content": text});
            if !thinking.is_empty() {
                m["thinking"] = json!(thinking);
            }
            if !images.is_empty() {
                m["images"] = Value::Array(images);
            }
            if !tool_calls.is_empty() {
                m["tool_calls"] = Value::Array(tool_calls);
            }
            messages.push(m);
        }
        for (name, content) in tool_results {
            let mut m = json!({"role": "tool", "content": content});
            if !name.is_empty() {
                m["tool_name"] = json!(name);
            }
            messages.push(m);
        }
    }
    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert("messages".into(), Value::Array(messages));
    body.insert("stream".into(), json!(false));
    if let Some(ref rf) = req.response_format {
        if matches!(rf, ResponseFormat::JsonObject) {
            body.insert("format".into(), json!("json"));
        }
    }
    if declare_tools {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            })
            .collect();
        body.insert("tools".into(), Value::Array(tools));
    }
    // `think` accepts a boolean or one of the model's advertised level names.
    match req.thinking.as_ref() {
        None => {}
        Some(t) => match t.effort.as_deref() {
            Some(effort @ ("minimal" | "low" | "medium" | "high")) => {
                body.insert("think".into(), json!(effort));
            }
            Some(_) => {
                return Err(LmpError::UnsupportedFeature {
                    protocol: "ollama".into(),
                    feature: "thinking_effort".into(),
                });
            }
            None => {
                body.insert("think".into(), json!(true));
            }
        },
    }
    let mut opts = Map::new();
    if let Some(temp) = req.temperature {
        opts.insert("temperature".into(), json!(temp));
    }
    if let Some(max) = req.max_tokens {
        opts.insert("num_predict".into(), json!(max));
    }
    if !opts.is_empty() {
        body.insert("options".into(), Value::Object(opts));
    }
    Ok(Value::Object(body))
}

fn from_ollama_chat(raw: &Value) -> Result<LmpResponse, LmpError> {
    let mut content = Vec::new();
    if let Some(th) = raw["message"]["thinking"].as_str() {
        if !th.is_empty() {
            content.push(ContentBlock::Thinking {
                thinking: th.to_string(),
                signature: None,
            });
        }
    }
    if let Some(txt) = raw["message"]["content"].as_str() {
        if !txt.is_empty() {
            content.push(ContentBlock::Text { text: txt.to_string(), signature: None });
        }
    }
    // Ollama returns `function.arguments` as an object and no call id, so the
    // positional index doubles as the LMP tool call id (see to_ollama_chat).
    if let Some(calls) = raw["message"]["tool_calls"].as_array() {
        for (position, call) in calls.iter().enumerate() {
            let function = &call["function"];
            let name = function["name"].as_str().unwrap_or("");
            let index = function["index"].as_u64().unwrap_or(position as u64);
            content.push(ContentBlock::ToolCall {
                id: format!("ollama-call:{index}:{name}"),
                name: name.to_string(),
                arguments: function["arguments"].clone(),
                signature: None,
            });
        }
    }
    let input_tokens = raw["prompt_eval_count"].as_u64().unwrap_or(0);
    let output_tokens = raw["eval_count"].as_u64().unwrap_or(0);
    Ok(LmpResponse {
        id: None,
        model: raw["model"].as_str().map(ToString::to_string),
        content,
        finish_reason: raw["done_reason"].as_str().map(ToString::to_string),
        usage: Usage {
            input_tokens,
            output_tokens,
            total_tokens: input_tokens + output_tokens,
            ..Default::default()
        },
        raw: raw.clone(),
    })
}

fn to_cohere_chat(req: &LmpRequest) -> Result<Value, LmpError> {
    let mut messages = Vec::new();
    for msg in &req.messages {
        let role = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        let mut text = String::new();
        let mut thinking = String::new();
        let mut images = Vec::new();
        let mut tool_calls = Vec::new();
        let mut tool_results = Vec::new();
        for b in &msg.content {
            match b {
                ContentBlock::Text { text: t, .. } => text.push_str(t),
                ContentBlock::Thinking { thinking: t, .. } => thinking.push_str(t),
                ContentBlock::ImageUrl { url } => {
                    images.push(json!({"type": "image_url", "image_url": {"url": url}}));
                }
                ContentBlock::ToolCall { id, name, arguments, .. } => {
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {"name": name, "arguments": arguments.to_string()}
                    }));
                }
                ContentBlock::ToolResult { tool_call_id, content, .. } => {
                    tool_results.push((tool_call_id.clone(), content.clone()));
                }
                ContentBlock::Audio { .. } => {
                    return Err(LmpError::UnsupportedFeature {
                        protocol: "cohere".into(),
                        feature: "audio_input".into(),
                    });
                }
                ContentBlock::ImageBase64 { .. } => {
                    return Err(LmpError::UnsupportedFeature {
                        protocol: "cohere".into(),
                        feature: "image_base64".into(),
                    });
                }
            }
        }
        if role == "assistant" {
            let mut m = Map::new();
            m.insert("role".into(), json!("assistant"));
            let mut blocks = Vec::new();
            if !thinking.is_empty() {
                blocks.push(json!({"type": "thinking", "thinking": thinking}));
            }
            if !text.is_empty() {
                blocks.push(json!({"type": "text", "text": text}));
            }
            if !blocks.is_empty() {
                m.insert("content".into(), Value::Array(blocks));
            }
            if !tool_calls.is_empty() {
                m.insert("tool_calls".into(), Value::Array(tool_calls));
            }
            messages.push(Value::Object(m));
        } else if images.is_empty() {
            if !text.is_empty() {
                messages.push(json!({"role": role, "content": text}));
            }
        } else {
            let mut blocks = Vec::new();
            if !text.is_empty() {
                blocks.push(json!({"type": "text", "text": text}));
            }
            blocks.extend(images);
            messages.push(json!({"role": role, "content": blocks}));
        }
        for (id, content) in tool_results {
            messages.push(json!({
                "role": "tool",
                "tool_call_id": id,
                "content": [{"type": "document", "document": {"data": content}}]
            }));
        }
    }
    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert("messages".into(), Value::Array(messages));
    if let Some(max) = req.max_tokens {
        body.insert("max_tokens".into(), json!(max));
    }
    if let Some(temp) = req.temperature {
        body.insert("temperature".into(), json!(temp));
    }
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            })
            .collect();
        body.insert("tools".into(), Value::Array(tools));
    }
    if let Some(ref tc) = req.tool_choice {
        // Cohere only models a binary switch (REQUIRED / NONE). `auto` is
        // Cohere's default, so it omits the field instead of sending NONE,
        // which would disable tool calling outright. `tool` has no
        // representation at all.
        let choice = match tc {
            ToolChoice::Required => Some(json!("REQUIRED")),
            ToolChoice::None => Some(json!("NONE")),
            ToolChoice::Auto => None,
            ToolChoice::Tool(_) => {
                return Err(LmpError::UnsupportedFeature {
                    protocol: "cohere".into(),
                    feature: "tool_choice_tool".into(),
                });
            }
        };
        if let Some(choice) = choice {
            if req.tools.is_empty() {
                return Err(LmpError::UnsupportedFeature {
                    protocol: "cohere".into(),
                    feature: "tool_choice_without_tools".into(),
                });
            }
            body.insert("tool_choice".into(), choice);
        }
    }
    if let Some(ref rf) = req.response_format {
        match rf {
            ResponseFormat::Text => {}
            ResponseFormat::JsonObject => {
                body.insert("response_format".into(), json!({"type": "json_object"}));
            }
            ResponseFormat::JsonSchema { schema, .. } => {
                body.insert(
                    "response_format".into(),
                    json!({"type": "json_object", "json_schema": schema}),
                );
            }
        }
    }
    if let Some(ref t) = req.thinking {
        let mut th = json!({"type": "enabled"});
        if let Some(budget) = t.budget_tokens {
            th["token_budget"] = json!(budget);
        }
        body.insert("thinking".into(), th);
    }
    Ok(Value::Object(body))
}

fn from_cohere_chat(raw: &Value) -> Result<LmpResponse, LmpError> {
    let mut content = Vec::new();
    if let Some(blocks) = raw["message"]["content"].as_array() {
        for b in blocks {
            match b["type"].as_str().unwrap_or("") {
                "thinking" => {
                    if let Some(th) = b["thinking"].as_str() {
                        content.push(ContentBlock::Thinking {
                            thinking: th.to_string(),
                            signature: None,
                        });
                    }
                }
                "text" => {
                    if let Some(txt) = b["text"].as_str() {
                        content.push(ContentBlock::Text { text: txt.to_string(), signature: None });
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(calls) = raw["message"]["tool_calls"].as_array() {
        for call in calls {
            let function = &call["function"];
            content.push(ContentBlock::ToolCall {
                id: call["id"].as_str().unwrap_or("").to_string(),
                name: function["name"].as_str().unwrap_or("").to_string(),
                arguments: function["arguments"]
                    .as_str()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_else(|| json!({})),
                signature: None,
            });
        }
    }
    let tokens = &raw["usage"]["tokens"];
    let input_tokens = tokens["input_tokens"].as_u64().unwrap_or(0);
    let output_tokens = tokens["output_tokens"].as_u64().unwrap_or(0);
    Ok(LmpResponse {
        id: raw["id"].as_str().map(ToString::to_string),
        model: None,
        content,
        finish_reason: raw["finish_reason"].as_str().map(ToString::to_string),
        usage: Usage {
            input_tokens,
            output_tokens,
            total_tokens: input_tokens + output_tokens,
            ..Default::default()
        },
        raw: raw.clone(),
    })
}
