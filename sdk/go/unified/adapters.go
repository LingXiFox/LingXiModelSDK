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
	case native.ProtocolOpenAIChatCompletions:
		payload, err = toOpenAIChat(req)
	case native.ProtocolOpenAIResponses:
		payload, err = toOpenAIResponses(req)
	case native.ProtocolAnthropicMessages:
		payload, err = toAnthropicMessages(req)
	case native.ProtocolGoogleGemini:
		payload, err = toGoogleGemini(req)
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
	case native.ProtocolOpenAIChatCompletions:
		return fromOpenAIChat(raw)
	case native.ProtocolOpenAIResponses:
		return fromOpenAIResponses(raw)
	case native.ProtocolAnthropicMessages:
		return fromAnthropicMessages(raw)
	case native.ProtocolGoogleGemini:
		return fromGoogleGemini(raw)
	default:
		return nil, &UnsupportedFeatureError{Protocol: string(proto), Feature: "protocol"}
	}
}

func NormalizeStreamEvents(proto native.Protocol, events []native.SSEEvent) ([]StreamEvent, error) {
	var out []StreamEvent
	for _, ev := range events {
		trimmed := strings.TrimSpace(ev.Data)
		if trimmed == "" || trimmed == "[DONE]" {
			continue
		}
		var val map[string]any
		if err := json.Unmarshal([]byte(trimmed), &val); err != nil {
			return nil, err
		}
		switch proto {
		case native.ProtocolOpenAIChatCompletions:
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
		case native.ProtocolGoogleGemini:
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
		var text string
		for _, b := range m.Content {
			if b.Type == "text" {
				text += b.Text
			}
		}
		msgs = append(msgs, map[string]any{"role": m.Role, "content": text})
	}
	out := map[string]any{"model": req.Model, "messages": msgs}
	if req.MaxTokens > 0 {
		out["max_tokens"] = req.MaxTokens
	}
	if req.Thinking != nil && req.Thinking.Effort != "" {
		out["reasoning_effort"] = req.Thinking.Effort
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
					content = append(content, ContentBlock{Type: "thinking", Thinking: txt})
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
			InputTokens:  int64Field(u, "promptTokenCount"),
			OutputTokens: int64Field(u, "candidatesTokenCount"),
			TotalTokens:  int64Field(u, "totalTokenCount"),
		},
		Raw: raw,
	}, nil
}
