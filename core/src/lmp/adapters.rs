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
        NativeProtocol::GoogleGemini | NativeProtocol::GoogleVertexAi => to_google_gemini(req)?,
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
    let mut out = Vec::new();
    for ev in events {
        if ev.data.trim() == "[DONE]" || ev.data.trim().is_empty() {
            continue;
        }
        let val: Value =
            serde_json::from_str(&ev.data).map_err(|e| LmpError::Decode(e.to_string()))?;
        match protocol {
            NativeProtocol::OpenaiChatCompletions | NativeProtocol::AzureOpenai => {
                if let Some(choice) = val["choices"].get(0) {
                    if let Some(delta) = choice["delta"].as_object() {
                        if let Some(rc) = delta.get("reasoning_content").and_then(Value::as_str) {
                            if !rc.is_empty() {
                                out.push(LmpStreamEvent::ThinkingDelta {
                                    delta: rc.to_string(),
                                });
                            }
                        }
                        if let Some(txt) = delta.get("content").and_then(Value::as_str) {
                            if !txt.is_empty() {
                                out.push(LmpStreamEvent::TextDelta {
                                    delta: txt.to_string(),
                                });
                            }
                        }
                    }
                    if let Some(fr) = choice["finish_reason"].as_str() {
                        out.push(LmpStreamEvent::MessageEnd {
                            finish_reason: Some(fr.to_string()),
                        });
                    }
                }
                if let Some(u) = val.get("usage").filter(|u| u.is_object()) {
                    out.push(LmpStreamEvent::Usage {
                        usage: parse_openai_usage(u),
                    });
                }
            }
            NativeProtocol::OpenaiResponses => {
                let kind = ev
                    .event
                    .as_deref()
                    .or_else(|| val["type"].as_str())
                    .unwrap_or("");
                match kind {
                    "response.created" => out.push(LmpStreamEvent::MessageStart {
                        id: val["response"]["id"].as_str().map(ToString::to_string),
                    }),
                    "response.output_text.delta" => {
                        if let Some(d) = val["delta"].as_str() {
                            out.push(LmpStreamEvent::TextDelta {
                                delta: d.to_string(),
                            });
                        }
                    }
                    "response.reasoning_summary_text.delta" => {
                        if let Some(d) = val["delta"].as_str() {
                            out.push(LmpStreamEvent::ThinkingDelta {
                                delta: d.to_string(),
                            });
                        }
                    }
                    "response.completed" => out.push(LmpStreamEvent::MessageEnd {
                        finish_reason: val["response"]["status"].as_str().map(ToString::to_string),
                    }),
                    _ => {}
                }
            }
            NativeProtocol::AnthropicMessages => {
                let kind = ev
                    .event
                    .as_deref()
                    .or_else(|| val["type"].as_str())
                    .unwrap_or("");
                match kind {
                    "message_start" => out.push(LmpStreamEvent::MessageStart {
                        id: val["message"]["id"].as_str().map(ToString::to_string),
                    }),
                    "content_block_delta" => {
                        let d = &val["delta"];
                        match d["type"].as_str() {
                            Some("thinking_delta") => {
                                if let Some(t) = d["thinking"].as_str() {
                                    out.push(LmpStreamEvent::ThinkingDelta {
                                        delta: t.to_string(),
                                    });
                                }
                            }
                            Some("text_delta") => {
                                if let Some(t) = d["text"].as_str() {
                                    out.push(LmpStreamEvent::TextDelta {
                                        delta: t.to_string(),
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                    "message_stop" => out.push(LmpStreamEvent::MessageEnd {
                        finish_reason: Some("stop".to_string()),
                    }),
                    _ => {}
                }
            }
            NativeProtocol::GoogleGemini | NativeProtocol::GoogleVertexAi => {
                if let Some(cand) = val["candidates"].get(0) {
                    if let Some(parts) = cand["content"]["parts"].as_array() {
                        for part in parts {
                            if let Some(txt) = part["text"].as_str() {
                                if part["thought"].as_bool() == Some(true) {
                                    out.push(LmpStreamEvent::ThinkingDelta {
                                        delta: txt.to_string(),
                                    });
                                } else {
                                    out.push(LmpStreamEvent::TextDelta {
                                        delta: txt.to_string(),
                                    });
                                }
                            }
                        }
                    }
                    if let Some(fr) = cand["finishReason"].as_str() {
                        out.push(LmpStreamEvent::MessageEnd {
                            finish_reason: Some(fr.to_string()),
                        });
                    }
                }
            }
            NativeProtocol::AwsBedrock => {
                if let Some(delta) = val["contentBlockDelta"]["delta"].as_object() {
                    if let Some(txt) = delta.get("text").and_then(Value::as_str) {
                        out.push(LmpStreamEvent::TextDelta {
                            delta: txt.to_string(),
                        });
                    }
                    if let Some(rt) = delta.get("reasoningContent").and_then(|v| v["text"].as_str()) {
                        out.push(LmpStreamEvent::ThinkingDelta {
                            delta: rt.to_string(),
                        });
                    }
                    if let Some(inp) = delta.get("toolUse").and_then(|v| v["input"].as_str()) {
                        let idx = val["contentBlockDelta"]["contentBlockIndex"].as_u64().unwrap_or(0) as usize;
                        out.push(LmpStreamEvent::ToolCallDelta {
                            index: idx,
                            id: None,
                            name: None,
                            arguments_delta: inp.to_string(),
                        });
                    }
                } else if let Some(start) = val["contentBlockStart"]["start"]["toolUse"].as_object() {
                    let idx = val["contentBlockStart"]["contentBlockIndex"].as_u64().unwrap_or(0) as usize;
                    out.push(LmpStreamEvent::ToolCallDelta {
                        index: idx,
                        id: start.get("toolUseId").and_then(Value::as_str).map(ToString::to_string),
                        name: start.get("name").and_then(Value::as_str).map(ToString::to_string),
                        arguments_delta: String::new(),
                    });
                } else if let Some(sr) = val["messageStop"]["stopReason"].as_str() {
                    out.push(LmpStreamEvent::MessageEnd {
                        finish_reason: Some(sr.to_string()),
                    });
                } else if let Some(u) = val["metadata"].get("usage").filter(|v| v.is_object()) {
                    out.push(LmpStreamEvent::Usage {
                        usage: Usage {
                            input_tokens: u["inputTokens"].as_u64().unwrap_or(0),
                            output_tokens: u["outputTokens"].as_u64().unwrap_or(0),
                            total_tokens: u["totalTokens"].as_u64().unwrap_or(0),
                            cache_read_tokens: u["cacheReadInputTokens"].as_u64(),
                            cache_write_tokens: u["cacheWriteInputTokens"].as_u64(),
                            ..Default::default()
                        },
                    });
                }
            }
            NativeProtocol::Ollama => {
                if let Some(th) = val["message"]["thinking"].as_str() {
                    if !th.is_empty() {
                        out.push(LmpStreamEvent::ThinkingDelta {
                            delta: th.to_string(),
                        });
                    }
                }
                if let Some(txt) = val["message"]["content"].as_str() {
                    if !txt.is_empty() {
                        out.push(LmpStreamEvent::TextDelta {
                            delta: txt.to_string(),
                        });
                    }
                }
                if val["done"].as_bool() == Some(true) {
                    let input_tokens = val["prompt_eval_count"].as_u64().unwrap_or(0);
                    let output_tokens = val["eval_count"].as_u64().unwrap_or(0);
                    out.push(LmpStreamEvent::Usage {
                        usage: Usage {
                            input_tokens,
                            output_tokens,
                            total_tokens: input_tokens + output_tokens,
                            ..Default::default()
                        },
                    });
                    out.push(LmpStreamEvent::MessageEnd {
                        finish_reason: val["done_reason"].as_str().map(ToString::to_string),
                    });
                }
            }
            NativeProtocol::Cohere => {
                let kind = ev.event.as_deref().or_else(|| val["type"].as_str()).unwrap_or("");
                match kind {
                    "content-delta" => {
                        if let Some(txt) = val["delta"]["message"]["content"]["text"].as_str() {
                            out.push(LmpStreamEvent::TextDelta {
                                delta: txt.to_string(),
                            });
                        }
                        if let Some(th) = val["delta"]["message"]["content"]["thinking"].as_str() {
                            out.push(LmpStreamEvent::ThinkingDelta {
                                delta: th.to_string(),
                            });
                        }
                    }
                    "message-end" => {
                        if let Some(u) = val["delta"]["usage"].get("tokens") {
                            let input_tokens = u["input_tokens"].as_u64().unwrap_or(0);
                            let output_tokens = u["output_tokens"].as_u64().unwrap_or(0);
                            out.push(LmpStreamEvent::Usage {
                                usage: Usage {
                                    input_tokens,
                                    output_tokens,
                                    total_tokens: input_tokens + output_tokens,
                                    ..Default::default()
                                },
                            });
                        }
                        out.push(LmpStreamEvent::MessageEnd {
                            finish_reason: val["delta"]["finish_reason"].as_str().map(ToString::to_string),
                        });
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(out)
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
        let mut tool_call_id = None;

        for block in &msg.content {
            match block {
                ContentBlock::Text { text } => {
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
                ContentBlock::ToolCall { id, name, arguments } => {
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
                    tool_call_id = Some(id.clone());
                    text_parts.push(content.clone());
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
        if let Some(id) = tool_call_id {
            m.insert("tool_call_id".into(), json!(id));
        }
        messages.push(Value::Object(m));
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
        let mut parts = Vec::new();
        for b in &msg.content {
            match b {
                ContentBlock::Text { text } => {
                    parts.push(json!({"type": "input_text", "text": text}));
                }
                ContentBlock::ImageUrl { url } => {
                    parts.push(json!({"type": "input_image", "image_url": url}));
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
                _ => {}
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
    let mut system_parts = Vec::new();
    let mut messages = Vec::new();
    for msg in &req.messages {
        if msg.role == Role::System {
            for b in &msg.content {
                if let ContentBlock::Text { text } = b {
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
                ContentBlock::Text { text } => blocks.push(json!({"type": "text", "text": text})),
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
    Ok(Value::Object(body))
}

fn to_google_gemini(req: &LmpRequest) -> Result<Value, LmpError> {
    let mut system_parts = Vec::new();
    let mut contents = Vec::new();
    for msg in &req.messages {
        if msg.role == Role::System {
            for b in &msg.content {
                if let ContentBlock::Text { text } = b {
                    system_parts.push(json!({"text": text}));
                }
            }
            continue;
        }
        let role = match msg.role {
            Role::Assistant => "model",
            _ => "user",
        };
        let mut parts = Vec::new();
        for b in &msg.content {
            match b {
                ContentBlock::Text { text } => parts.push(json!({"text": text})),
                ContentBlock::ImageBase64 { media_type, data } => {
                    parts.push(json!({"inlineData": {"mimeType": media_type, "data": data}}))
                }
                ContentBlock::Audio { media_type, data } => {
                    parts.push(json!({"inlineData": {"mimeType": media_type, "data": data}}))
                }
                ContentBlock::ImageUrl { .. } => {
                    return Err(LmpError::UnsupportedFeature {
                        protocol: "google_gemini".into(),
                        feature: "image_url".into(),
                    });
                }
                ContentBlock::Thinking { .. } => {}
                ContentBlock::ToolCall { name, arguments, .. } => {
                    parts.push(json!({"functionCall": {"name": name, "args": arguments}}));
                }
                ContentBlock::ToolResult {
                    tool_call_id,
                    content,
                    ..
                } => {
                    parts.push(json!({
                        "functionResponse": {"name": tool_call_id, "response": {"content": content}}
                    }));
                }
            }
        }
        contents.push(json!({"role": role, "parts": parts}));
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
        if let Some(budget) = t.budget_tokens {
            gen_cfg.insert("thinkingConfig".into(), json!({"thinkingBudget": budget}));
        } else if let Some(ref effort) = t.effort {
            gen_cfg.insert("thinkingConfig".into(), json!({"thinkingLevel": effort}));
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
                        if !text.is_empty() {
                            content.push(ContentBlock::Thinking {
                                thinking: text,
                                signature: None,
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
                }),
                Some("tool_use") => content.push(ContentBlock::ToolCall {
                    id: b["id"].as_str().unwrap_or("").to_string(),
                    name: b["name"].as_str().unwrap_or("").to_string(),
                    arguments: b["input"].clone(),
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
        for p in parts {
            if let Some(txt) = p["text"].as_str() {
                if p["thought"].as_bool() == Some(true) {
                    content.push(ContentBlock::Thinking {
                        thinking: txt.to_string(),
                        signature: p["thoughtSignature"].as_str().map(ToString::to_string),
                    });
                } else {
                    content.push(ContentBlock::Text {
                        text: txt.to_string(),
                    });
                }
            } else if p["functionCall"].is_object() {
                content.push(ContentBlock::ToolCall {
                    id: p["functionCall"]["name"].as_str().unwrap_or("").to_string(),
                    name: p["functionCall"]["name"].as_str().unwrap_or("").to_string(),
                    arguments: p["functionCall"]["args"].clone(),
                });
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
                if let ContentBlock::Text { text } = b {
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
                ContentBlock::Text { text } => blocks.push(json!({"text": text})),
                ContentBlock::Thinking { thinking, signature } => {
                    let mut rc = json!({"text": thinking});
                    if let Some(sig) = signature {
                        rc["signature"] = json!(sig);
                    }
                    blocks.push(json!({"reasoningContent": {"reasoningText": rc}}));
                }
                ContentBlock::ToolCall { id, name, arguments } => {
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
                ContentBlock::Audio { .. } => {
                    return Err(LmpError::UnsupportedFeature {
                        protocol: "aws_bedrock".into(),
                        feature: "audio_input".into(),
                    });
                }
                _ => {}
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
        body.insert("toolConfig".into(), json!({"tools": tools}));
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
                content.push(ContentBlock::Text { text: txt.to_string() });
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
    let mut messages = Vec::new();
    for msg in &req.messages {
        let role = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        let mut text = String::new();
        let mut images = Vec::new();
        for b in &msg.content {
            match b {
                ContentBlock::Text { text: t } => text.push_str(t),
                ContentBlock::ImageBase64 { data, .. } => images.push(json!(data)),
                ContentBlock::ToolResult { content, .. } => text.push_str(content),
                ContentBlock::Audio { .. } => {
                    return Err(LmpError::UnsupportedFeature {
                        protocol: "ollama".into(),
                        feature: "audio_input".into(),
                    });
                }
                _ => {}
            }
        }
        let mut m = json!({"role": role, "content": text});
        if !images.is_empty() {
            m["images"] = Value::Array(images);
        }
        messages.push(m);
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
    if req.thinking.is_some() {
        body.insert("think".into(), json!(true));
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
            content.push(ContentBlock::Text { text: txt.to_string() });
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
        for b in &msg.content {
            match b {
                ContentBlock::Text { text: t } => text.push_str(t),
                ContentBlock::ToolResult { content, .. } => text.push_str(content),
                ContentBlock::Audio { .. } => {
                    return Err(LmpError::UnsupportedFeature {
                        protocol: "cohere".into(),
                        feature: "audio_input".into(),
                    });
                }
                _ => {}
            }
        }
        messages.push(json!({"role": role, "content": text}));
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
                        content.push(ContentBlock::Text { text: txt.to_string() });
                    }
                }
                _ => {}
            }
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
