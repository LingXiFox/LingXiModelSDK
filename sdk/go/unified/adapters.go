package unified

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/LingXiFox/LingXiModelSDK/sdk/go/native"
)

func TranslateRequest(proto native.Protocol, req Request) (map[string]any, error) {
	var payload map[string]any
	var err error
	switch proto {
	case native.ProtocolOpenAIChatCompletions, native.ProtocolAzureOpenAI:
		payload, err = toOpenAIChat(req)
	case native.ProtocolOpenAIResponses:
		payload, err = toOpenAIResponses(req)
	case native.ProtocolAnthropicMessages:
		payload, err = toAnthropicMessages(req)
	case native.ProtocolGoogleGemini, native.ProtocolGoogleVertexAI:
		payload, err = toGoogleGemini(req)
	case native.ProtocolAWSBedrock:
		payload, err = toAWSBedrock(req)
	case native.ProtocolOllama:
		payload, err = toOllamaChat(req)
	case native.ProtocolCohere:
		payload, err = toCohereChat(req)
	default:
		return nil, &UnsupportedFeatureError{Protocol: string(proto), Feature: "protocol"}
	}
	if err != nil {
		return nil, err
	}
	for k, v := range req.ProviderExtensions {
		payload[k] = v
	}
	return payload, nil
}

func NormalizeResponse(proto native.Protocol, raw map[string]any) (*Response, error) {
	switch proto {
	case native.ProtocolOpenAIChatCompletions, native.ProtocolAzureOpenAI:
		return fromOpenAIChat(raw)
	case native.ProtocolOpenAIResponses:
		return fromOpenAIResponses(raw)
	case native.ProtocolAnthropicMessages:
		return fromAnthropicMessages(raw)
	case native.ProtocolGoogleGemini, native.ProtocolGoogleVertexAI:
		return fromGoogleGemini(raw)
	case native.ProtocolAWSBedrock:
		return fromAWSBedrock(raw)
	case native.ProtocolOllama:
		return fromOllamaChat(raw)
	case native.ProtocolCohere:
		return fromCohereChat(raw)
	default:
		return nil, &UnsupportedFeatureError{Protocol: string(proto), Feature: "protocol"}
	}
}

func NormalizeStreamEvents(proto native.Protocol, events []native.SSEEvent) ([]StreamEvent, error) {
	var out []StreamEvent
	for _, ev := range events {
		parsed, err := NormalizeStreamEvent(proto, ev)
		if err != nil {
			return nil, err
		}
		out = append(out, parsed...)
	}
	return out, nil
}

// NormalizeStreamEvent parses a single SSE/NDJSON frame into zero or more LMP StreamEvents.
func NormalizeStreamEvent(proto native.Protocol, ev native.SSEEvent) ([]StreamEvent, error) {
	var out []StreamEvent
	trimmed := strings.TrimSpace(ev.Data)
	if trimmed == "" || trimmed == "[DONE]" {
		return out, nil
	}
	var val map[string]any
	if err := json.Unmarshal([]byte(trimmed), &val); err != nil {
		return nil, err
	}
	switch proto {
	case native.ProtocolOpenAIChatCompletions, native.ProtocolAzureOpenAI:
		if choices, ok := val["choices"].([]any); ok && len(choices) > 0 {
			if c, ok := choices[0].(map[string]any); ok {
				if d, ok := c["delta"].(map[string]any); ok {
					if rc, _ := d["reasoning_content"].(string); rc != "" {
						out = append(out, StreamEvent{Type: "thinking_delta", Delta: rc})
					}
					if txt, _ := d["content"].(string); txt != "" {
						out = append(out, StreamEvent{Type: "text_delta", Delta: txt})
					}
				}
				if fr, _ := c["finish_reason"].(string); fr != "" {
					out = append(out, StreamEvent{Type: "message_end", FinishReason: fr})
				}
			}
		}
	case native.ProtocolOpenAIResponses:
		kind := ev.Event
		if kind == "" {
			kind, _ = val["type"].(string)
		}
		switch kind {
		case "response.output_text.delta":
			if d, _ := val["delta"].(string); d != "" {
				out = append(out, StreamEvent{Type: "text_delta", Delta: d})
			}
		case "response.reasoning_summary_text.delta":
			if d, _ := val["delta"].(string); d != "" {
				out = append(out, StreamEvent{Type: "thinking_delta", Delta: d})
			}
		case "response.completed":
			out = append(out, StreamEvent{Type: "message_end", FinishReason: "completed"})
		}
	case native.ProtocolAnthropicMessages:
		kind := ev.Event
		if kind == "" {
			kind, _ = val["type"].(string)
		}
		switch kind {
		case "content_block_delta":
			if d, ok := val["delta"].(map[string]any); ok {
				switch d["type"] {
				case "thinking_delta":
					if t, _ := d["thinking"].(string); t != "" {
						out = append(out, StreamEvent{Type: "thinking_delta", Delta: t})
					}
				case "text_delta":
					if t, _ := d["text"].(string); t != "" {
						out = append(out, StreamEvent{Type: "text_delta", Delta: t})
					}
				}
			}
		case "message_stop":
			out = append(out, StreamEvent{Type: "message_end", FinishReason: "stop"})
		}
	case native.ProtocolGoogleGemini, native.ProtocolGoogleVertexAI:
		if cands, ok := val["candidates"].([]any); ok && len(cands) > 0 {
			if c, ok := cands[0].(map[string]any); ok {
				if content, ok := c["content"].(map[string]any); ok {
					if parts, ok := content["parts"].([]any); ok {
						for _, rawP := range parts {
							if p, ok := rawP.(map[string]any); ok {
								if txt, _ := p["text"].(string); txt != "" {
									if th, _ := p["thought"].(bool); th {
										out = append(out, StreamEvent{Type: "thinking_delta", Delta: txt})
									} else {
										out = append(out, StreamEvent{Type: "text_delta", Delta: txt})
									}
								}
							}
						}
					}
				}
				if fr, _ := c["finishReason"].(string); fr != "" {
					out = append(out, StreamEvent{Type: "message_end", FinishReason: fr})
				}
			}
		}
	case native.ProtocolAWSBedrock:
		if cb, ok := val["contentBlockDelta"].(map[string]any); ok {
			if d, ok := cb["delta"].(map[string]any); ok {
				if txt, _ := d["text"].(string); txt != "" {
					out = append(out, StreamEvent{Type: "text_delta", Delta: txt})
				}
				if rc, ok := d["reasoningContent"].(map[string]any); ok {
					if txt, _ := rc["text"].(string); txt != "" {
						out = append(out, StreamEvent{Type: "thinking_delta", Delta: txt})
					}
				}
			}
		} else if ms, ok := val["messageStop"].(map[string]any); ok {
			sr, _ := ms["stopReason"].(string)
			out = append(out, StreamEvent{Type: "message_end", FinishReason: sr})
		}
	case native.ProtocolOllama:
		if msg, ok := val["message"].(map[string]any); ok {
			if th, _ := msg["thinking"].(string); th != "" {
				out = append(out, StreamEvent{Type: "thinking_delta", Delta: th})
			}
			if txt, _ := msg["content"].(string); txt != "" {
				out = append(out, StreamEvent{Type: "text_delta", Delta: txt})
			}
		}
		if done, _ := val["done"].(bool); done {
			fr, _ := val["done_reason"].(string)
			out = append(out, StreamEvent{Type: "message_end", FinishReason: fr})
		}
	case native.ProtocolCohere:
		kind := ev.Event
		if kind == "" {
			kind, _ = val["type"].(string)
		}
		switch kind {
		case "content-delta":
			if d, ok := val["delta"].(map[string]any); ok {
				if m, ok := d["message"].(map[string]any); ok {
					if c, ok := m["content"].(map[string]any); ok {
						if txt, _ := c["text"].(string); txt != "" {
							out = append(out, StreamEvent{Type: "text_delta", Delta: txt})
						}
						if th, _ := c["thinking"].(string); th != "" {
							out = append(out, StreamEvent{Type: "thinking_delta", Delta: th})
						}
					}
				}
			}
		case "message-end":
			fr := ""
			if d, ok := val["delta"].(map[string]any); ok {
				fr, _ = d["finish_reason"].(string)
			}
			out = append(out, StreamEvent{Type: "message_end", FinishReason: fr})
		}
	}
	return out, nil
}

func toOpenAIChat(req Request) (map[string]any, error) {
	if req.Thinking != nil && req.Thinking.BudgetTokens > 0 {
		return nil, &UnsupportedFeatureError{Protocol: string(native.ProtocolOpenAIChatCompletions), Feature: "budget_tokens"}
	}
	msgs := make([]map[string]any, 0, len(req.Messages))
	for _, m := range req.Messages {
		var text strings.Builder
		var parts []map[string]any
		var toolCalls []map[string]any
		var toolCallID string
		hasParts := false
		for _, b := range m.Content {
			switch b.Type {
			case "text":
				text.WriteString(b.Text)
				parts = append(parts, map[string]any{"type": "text", "text": b.Text})
			case "image_url":
				hasParts = true
				parts = append(parts, map[string]any{"type": "image_url", "image_url": map[string]any{"url": b.URL}})
			case "image_base64":
				hasParts = true
				parts = append(parts, map[string]any{"type": "image_url", "image_url": map[string]any{"url": "data:" + b.MediaType + ";base64," + b.Data}})
			case "tool_call":
				argBytes, _ := json.Marshal(b.Arguments)
				toolCalls = append(toolCalls, map[string]any{
					"id":       b.ID,
					"type":     "function",
					"function": map[string]any{"name": b.Name, "arguments": string(argBytes)},
				})
			case "tool_result":
				toolCallID = b.ToolCallID
				text.WriteString(b.Content)
			case "audio":
				return nil, &UnsupportedFeatureError{Protocol: "openai_chat_completions", Feature: "audio_input"}
			}
		}
		entry := map[string]any{"role": m.Role}
		if hasParts {
			entry["content"] = parts
		} else {
			entry["content"] = text.String()
		}
		if len(toolCalls) > 0 {
			entry["tool_calls"] = toolCalls
		}
		if toolCallID != "" {
			entry["tool_call_id"] = toolCallID
		}
		msgs = append(msgs, entry)
	}
	out := map[string]any{"model": req.Model, "messages": msgs}
	if req.MaxTokens > 0 {
		out["max_tokens"] = req.MaxTokens
	}
	if req.Temperature != nil {
		out["temperature"] = *req.Temperature
	}
	if req.Thinking != nil && req.Thinking.Effort != "" {
		out["reasoning_effort"] = req.Thinking.Effort
	}
	if len(req.Tools) > 0 {
		var tools []map[string]any
		for _, t := range req.Tools {
			tools = append(tools, map[string]any{
				"type":     "function",
				"function": map[string]any{"name": t.Name, "description": t.Description, "parameters": t.Parameters},
			})
		}
		out["tools"] = tools
	}
	if req.ToolChoice != nil {
		out["tool_choice"] = req.ToolChoice
	}
	return out, nil
}

func toOpenAIResponses(req Request) (map[string]any, error) {
	if req.Thinking != nil && req.Thinking.BudgetTokens > 0 {
		return nil, &UnsupportedFeatureError{Protocol: string(native.ProtocolOpenAIResponses), Feature: "budget_tokens"}
	}
	input := make([]map[string]any, 0, len(req.Messages))
	for _, m := range req.Messages {
		parts := make([]map[string]any, 0, len(m.Content))
		for _, b := range m.Content {
			if b.Type == "text" {
				parts = append(parts, map[string]any{"type": "input_text", "text": b.Text})
			}
		}
		input = append(input, map[string]any{"role": m.Role, "content": parts})
	}
	out := map[string]any{"model": req.Model, "input": input}
	if req.MaxTokens > 0 {
		out["max_output_tokens"] = req.MaxTokens
	}
	return out, nil
}

func toAnthropicMessages(req Request) (map[string]any, error) {
	if req.ResponseFormat != nil && (req.ResponseFormat.Type == "json_schema" || req.ResponseFormat.Type == "json_object") {
		return nil, &UnsupportedFeatureError{Protocol: string(native.ProtocolAnthropicMessages), Feature: "json_schema"}
	}
	var sys []string
	msgs := make([]map[string]any, 0, len(req.Messages))
	for _, m := range req.Messages {
		if m.Role == "system" {
			for _, b := range m.Content {
				if b.Type == "text" {
					sys = append(sys, b.Text)
				}
			}
			continue
		}
		blocks := make([]map[string]any, 0, len(m.Content))
		for _, b := range m.Content {
			if b.Type == "text" {
				blocks = append(blocks, map[string]any{"type": "text", "text": b.Text})
			}
		}
		msgs = append(msgs, map[string]any{"role": m.Role, "content": blocks})
	}
	maxTok := req.MaxTokens
	if maxTok <= 0 {
		maxTok = 4096
	}
	out := map[string]any{"model": req.Model, "max_tokens": maxTok, "messages": msgs}
	if len(sys) > 0 {
		out["system"] = strings.Join(sys, "\n")
	}
	if req.Thinking != nil && req.Thinking.BudgetTokens > 0 {
		out["thinking"] = map[string]any{"type": "enabled", "budget_tokens": req.Thinking.BudgetTokens}
	}
	return out, nil
}

func toGoogleGemini(req Request) (map[string]any, error) {
	contents := make([]map[string]any, 0, len(req.Messages))
	for _, m := range req.Messages {
		if m.Role == "system" {
			continue
		}
		role := "user"
		if m.Role == "assistant" {
			role = "model"
		}
		parts := make([]map[string]any, 0, len(m.Content))
		for _, b := range m.Content {
			if b.Type == "text" {
				parts = append(parts, map[string]any{"text": b.Text})
			}
		}
		contents = append(contents, map[string]any{"role": role, "parts": parts})
	}
	return map[string]any{"contents": contents}, nil
}

func int64Field(m map[string]any, key string) int64 {
	if m == nil {
		return 0
	}
	if f, ok := m[key].(float64); ok {
		return int64(f)
	}
	return 0
}

func optInt64Field(m map[string]any, key string) *int64 {
	if m == nil {
		return nil
	}
	if f, ok := m[key].(float64); ok {
		v := int64(f)
		return &v
	}
	return nil
}

func fromOpenAIChat(raw map[string]any) (*Response, error) {
	choices, ok := raw["choices"].([]any)
	if !ok || len(choices) == 0 {
		return nil, fmt.Errorf("missing choices[0]")
	}
	c0, _ := choices[0].(map[string]any)
	msg, _ := c0["message"].(map[string]any)
	var content []ContentBlock
	if rc, _ := msg["reasoning_content"].(string); rc != "" {
		content = append(content, ContentBlock{Type: "thinking", Thinking: rc})
	}
	if txt, _ := msg["content"].(string); txt != "" {
		content = append(content, ContentBlock{Type: "text", Text: txt})
	}
	u, _ := raw["usage"].(map[string]any)
	id, _ := raw["id"].(string)
	model, _ := raw["model"].(string)
	fr, _ := c0["finish_reason"].(string)
	return &Response{
		ID: id, Model: model, Content: content, FinishReason: fr,
		Usage: Usage{
			InputTokens:  int64Field(u, "prompt_tokens"),
			OutputTokens: int64Field(u, "completion_tokens"),
			TotalTokens:  int64Field(u, "total_tokens"),
		},
		Raw: raw,
	}, nil
}

func fromOpenAIResponses(raw map[string]any) (*Response, error) {
	var content []ContentBlock
	if output, ok := raw["output"].([]any); ok {
		for _, itemRaw := range output {
			item, _ := itemRaw.(map[string]any)
			switch item["type"] {
			case "reasoning":
				if sum, ok := item["summary"].([]any); ok {
					var parts []string
					for _, s := range sum {
						if sm, ok := s.(map[string]any); ok {
							if t, _ := sm["text"].(string); t != "" {
								parts = append(parts, t)
							}
						}
					}
					if len(parts) > 0 {
						content = append(content, ContentBlock{Type: "thinking", Thinking: strings.Join(parts, "\n")})
					}
				}
			case "message":
				if parts, ok := item["content"].([]any); ok {
					for _, p := range parts {
						if pm, ok := p.(map[string]any); ok {
							if t, _ := pm["text"].(string); t != "" {
								content = append(content, ContentBlock{Type: "text", Text: t})
							}
						}
					}
				}
			}
		}
	}
	u, _ := raw["usage"].(map[string]any)
	id, _ := raw["id"].(string)
	model, _ := raw["model"].(string)
	fr, _ := raw["status"].(string)
	return &Response{
		ID: id, Model: model, Content: content, FinishReason: fr,
		Usage: Usage{
			InputTokens:  int64Field(u, "input_tokens"),
			OutputTokens: int64Field(u, "output_tokens"),
			TotalTokens:  int64Field(u, "total_tokens"),
		},
		Raw: raw,
	}, nil
}

func fromAnthropicMessages(raw map[string]any) (*Response, error) {
	var content []ContentBlock
	if blocks, ok := raw["content"].([]any); ok {
		for _, bRaw := range blocks {
			b, _ := bRaw.(map[string]any)
			switch b["type"] {
			case "thinking":
				th, _ := b["thinking"].(string)
				sig, _ := b["signature"].(string)
				content = append(content, ContentBlock{Type: "thinking", Thinking: th, Signature: sig})
			case "text":
				t, _ := b["text"].(string)
				content = append(content, ContentBlock{Type: "text", Text: t})
			}
		}
	}
	u, _ := raw["usage"].(map[string]any)
	inTok := int64Field(u, "input_tokens")
	outTok := int64Field(u, "output_tokens")
	id, _ := raw["id"].(string)
	model, _ := raw["model"].(string)
	fr, _ := raw["stop_reason"].(string)
	return &Response{
		ID: id, Model: model, Content: content, FinishReason: fr,
		Usage: Usage{
			InputTokens:      inTok,
			OutputTokens:     outTok,
			TotalTokens:      inTok + outTok,
			CacheReadTokens:  optInt64Field(u, "cache_read_input_tokens"),
			CacheWriteTokens: optInt64Field(u, "cache_creation_input_tokens"),
		},
		Raw: raw,
	}, nil
}

func fromGoogleGemini(raw map[string]any) (*Response, error) {
	cands, ok := raw["candidates"].([]any)
	if !ok || len(cands) == 0 {
		return nil, fmt.Errorf("missing candidates[0]")
	}
	c0, _ := cands[0].(map[string]any)
	cm, _ := c0["content"].(map[string]any)
	var content []ContentBlock
	if parts, ok := cm["parts"].([]any); ok {
		for _, pRaw := range parts {
			p, _ := pRaw.(map[string]any)
			if txt, _ := p["text"].(string); txt != "" {
				if th, _ := p["thought"].(bool); th {
					sig, _ := p["thoughtSignature"].(string)
					content = append(content, ContentBlock{Type: "thinking", Thinking: txt, Signature: sig})
				} else {
					content = append(content, ContentBlock{Type: "text", Text: txt})
				}
			}
		}
	}
	u, _ := raw["usageMetadata"].(map[string]any)
	fr, _ := c0["finishReason"].(string)
	return &Response{
		Content: content, FinishReason: fr,
		Usage: Usage{
			InputTokens:     int64Field(u, "promptTokenCount"),
			OutputTokens:    int64Field(u, "candidatesTokenCount"),
			TotalTokens:     int64Field(u, "totalTokenCount"),
			ReasoningTokens: optInt64Field(u, "thoughtsTokenCount"),
			CacheReadTokens: optInt64Field(u, "cachedContentTokenCount"),
		},
		Raw: raw,
	}, nil
}

func toAWSBedrock(req Request) (map[string]any, error) {
	var system []map[string]any
	var messages []map[string]any
	for _, m := range req.Messages {
		if m.Role == "system" {
			for _, b := range m.Content {
				if b.Type == "text" {
					system = append(system, map[string]any{"text": b.Text})
				}
			}
			continue
		}
		role := "user"
		if m.Role == "assistant" {
			role = "assistant"
		}
		var blocks []map[string]any
		for _, b := range m.Content {
			switch b.Type {
			case "text":
				blocks = append(blocks, map[string]any{"text": b.Text})
			case "thinking":
				rt := map[string]any{"text": b.Thinking}
				if b.Signature != "" {
					rt["signature"] = b.Signature
				}
				blocks = append(blocks, map[string]any{"reasoningContent": map[string]any{"reasoningText": rt}})
			case "tool_call":
				blocks = append(blocks, map[string]any{"toolUse": map[string]any{"toolUseId": b.ID, "name": b.Name, "input": b.Arguments}})
			case "tool_result":
				status := "success"
				if b.IsError {
					status = "error"
				}
				blocks = append(blocks, map[string]any{"toolResult": map[string]any{"toolUseId": b.ToolCallID, "content": []map[string]any{{"text": b.Content}}, "status": status}})
			case "audio":
				return nil, &UnsupportedFeatureError{Protocol: "aws_bedrock", Feature: "audio_input"}
			}
		}
		messages = append(messages, map[string]any{"role": role, "content": blocks})
	}
	body := map[string]any{"messages": messages}
	if len(system) > 0 {
		body["system"] = system
	}
	if req.MaxTokens > 0 || req.Temperature != nil {
		inf := map[string]any{}
		if req.MaxTokens > 0 {
			inf["maxTokens"] = req.MaxTokens
		}
		if req.Temperature != nil {
			inf["temperature"] = *req.Temperature
		}
		body["inferenceConfig"] = inf
	}
	if len(req.Tools) > 0 {
		var tools []map[string]any
		for _, t := range req.Tools {
			tools = append(tools, map[string]any{"toolSpec": map[string]any{"name": t.Name, "description": t.Description, "inputSchema": map[string]any{"json": t.Parameters}}})
		}
		body["toolConfig"] = map[string]any{"tools": tools}
	}
	if req.Thinking != nil && req.Thinking.BudgetTokens > 0 {
		body["additionalModelRequestFields"] = map[string]any{"thinking": map[string]any{"type": "enabled", "budget_tokens": req.Thinking.BudgetTokens}}
	}
	return body, nil
}

func fromAWSBedrock(raw map[string]any) (*Response, error) {
	var content []ContentBlock
	if out, ok := raw["output"].(map[string]any); ok {
		if msg, ok := out["message"].(map[string]any); ok {
			if blocks, ok := msg["content"].([]any); ok {
				for _, bRaw := range blocks {
					b, _ := bRaw.(map[string]any)
					if txt, _ := b["text"].(string); txt != "" {
						content = append(content, ContentBlock{Type: "text", Text: txt})
					} else if rc, ok := b["reasoningContent"].(map[string]any); ok {
						if rt, ok := rc["reasoningText"].(map[string]any); ok {
							th, _ := rt["text"].(string)
							sig, _ := rt["signature"].(string)
							content = append(content, ContentBlock{Type: "thinking", Thinking: th, Signature: sig})
						}
					} else if tu, ok := b["toolUse"].(map[string]any); ok {
						id, _ := tu["toolUseId"].(string)
						name, _ := tu["name"].(string)
						args, _ := tu["input"].(map[string]any)
						content = append(content, ContentBlock{Type: "tool_call", ID: id, Name: name, Arguments: args})
					}
				}
			}
		}
	}
	u, _ := raw["usage"].(map[string]any)
	fr, _ := raw["stopReason"].(string)
	return &Response{
		Content: content, FinishReason: fr,
		Usage: Usage{
			InputTokens:      int64Field(u, "inputTokens"),
			OutputTokens:     int64Field(u, "outputTokens"),
			TotalTokens:      int64Field(u, "totalTokens"),
			CacheReadTokens:  optInt64Field(u, "cacheReadInputTokens"),
			CacheWriteTokens: optInt64Field(u, "cacheWriteInputTokens"),
		},
		Raw: raw,
	}, nil
}

func toOllamaChat(req Request) (map[string]any, error) {
	if req.ResponseFormat != nil && req.ResponseFormat.Type == "json_schema" {
		return nil, &UnsupportedFeatureError{Protocol: "ollama", Feature: "json_schema_strict"}
	}
	var messages []map[string]any
	for _, m := range req.Messages {
		var sb strings.Builder
		var images []string
		for _, b := range m.Content {
			switch b.Type {
			case "text":
				sb.WriteString(b.Text)
			case "image_base64":
				images = append(images, b.Data)
			case "tool_result":
				sb.WriteString(b.Content)
			case "audio":
				return nil, &UnsupportedFeatureError{Protocol: "ollama", Feature: "audio_input"}
			}
		}
		entry := map[string]any{"role": m.Role, "content": sb.String()}
		if len(images) > 0 {
			entry["images"] = images
		}
		messages = append(messages, entry)
	}
	body := map[string]any{"model": req.Model, "messages": messages, "stream": false}
	if req.ResponseFormat != nil && req.ResponseFormat.Type == "json_object" {
		body["format"] = "json"
	}
	if req.Thinking != nil {
		body["think"] = true
	}
	return body, nil
}

func fromOllamaChat(raw map[string]any) (*Response, error) {
	var content []ContentBlock
	if msg, ok := raw["message"].(map[string]any); ok {
		if th, _ := msg["thinking"].(string); th != "" {
			content = append(content, ContentBlock{Type: "thinking", Thinking: th})
		}
		if txt, _ := msg["content"].(string); txt != "" {
			content = append(content, ContentBlock{Type: "text", Text: txt})
		}
	}
	inTok := int64Field(raw, "prompt_eval_count")
	outTok := int64Field(raw, "eval_count")
	model, _ := raw["model"].(string)
	fr, _ := raw["done_reason"].(string)
	return &Response{
		Model: model, Content: content, FinishReason: fr,
		Usage: Usage{InputTokens: inTok, OutputTokens: outTok, TotalTokens: inTok + outTok},
		Raw:   raw,
	}, nil
}

func toCohereChat(req Request) (map[string]any, error) {
	var messages []map[string]any
	for _, m := range req.Messages {
		var sb strings.Builder
		for _, b := range m.Content {
			switch b.Type {
			case "text":
				sb.WriteString(b.Text)
			case "tool_result":
				sb.WriteString(b.Content)
			case "audio":
				return nil, &UnsupportedFeatureError{Protocol: "cohere", Feature: "audio_input"}
			}
		}
		messages = append(messages, map[string]any{"role": m.Role, "content": sb.String()})
	}
	body := map[string]any{"model": req.Model, "messages": messages}
	if req.MaxTokens > 0 {
		body["max_tokens"] = req.MaxTokens
	}
	if req.Temperature != nil {
		body["temperature"] = *req.Temperature
	}
	if req.Thinking != nil {
		th := map[string]any{"type": "enabled"}
		if req.Thinking.BudgetTokens > 0 {
			th["token_budget"] = req.Thinking.BudgetTokens
		}
		body["thinking"] = th
	}
	return body, nil
}

func fromCohereChat(raw map[string]any) (*Response, error) {
	var content []ContentBlock
	if msg, ok := raw["message"].(map[string]any); ok {
		if blocks, ok := msg["content"].([]any); ok {
			for _, bRaw := range blocks {
				b, _ := bRaw.(map[string]any)
				switch b["type"] {
				case "thinking":
					if th, _ := b["thinking"].(string); th != "" {
						content = append(content, ContentBlock{Type: "thinking", Thinking: th})
					}
				case "text":
					if txt, _ := b["text"].(string); txt != "" {
						content = append(content, ContentBlock{Type: "text", Text: txt})
					}
				}
			}
		}
	}
	u, _ := raw["usage"].(map[string]any)
	tokens, _ := u["tokens"].(map[string]any)
	inTok := int64Field(tokens, "input_tokens")
	outTok := int64Field(tokens, "output_tokens")
	id, _ := raw["id"].(string)
	fr, _ := raw["finish_reason"].(string)
	return &Response{
		ID: id, Content: content, FinishReason: fr,
		Usage: Usage{InputTokens: inTok, OutputTokens: outTok, TotalTokens: inTok + outTok},
		Raw:   raw,
	}, nil
}
