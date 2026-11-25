package unified

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/LingXiFox/LingXiModelSDK/sdk/go/native"
)

// TranslateRequest converts an LMP request into the native wire payload of a
// protocol. Every mapping decision lives here so the adapters stay the single
// source of truth, exactly like the Rust core.
func TranslateRequest(proto native.Protocol, req Request) (map[string]any, error) {
	var payload map[string]any
	var err error
	switch proto {
	case native.ProtocolOpenAIChatCompletions, native.ProtocolAzureOpenAI:
		payload, err = toOpenAIChat(proto, req)
	case native.ProtocolOpenAIResponses:
		payload, err = toOpenAIResponses(req)
	case native.ProtocolAnthropicMessages:
		payload, err = toAnthropicMessages(req)
	case native.ProtocolGoogleGemini, native.ProtocolGoogleVertexAI:
		payload, err = toGoogleGemini(proto, req)
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

func unsupported(proto native.Protocol, feature string) error {
	return &UnsupportedFeatureError{Protocol: string(proto), Feature: feature}
}

// lookup walks a nested JSON object path, returning nil when a step is missing.
func lookup(raw any, path ...string) any {
	cur := raw
	for _, key := range path {
		m, ok := cur.(map[string]any)
		if !ok {
			return nil
		}
		cur = m[key]
	}
	return cur
}

func strAt(raw any, path ...string) string {
	s, _ := lookup(raw, path...).(string)
	return s
}

func objAt(raw any, path ...string) map[string]any {
	m, _ := lookup(raw, path...).(map[string]any)
	return m
}

func arrAt(raw any, path ...string) []any {
	a, _ := lookup(raw, path...).([]any)
	return a
}

func intAt(raw any, path ...string) (int64, bool) {
	f, ok := lookup(raw, path...).(float64)
	if !ok {
		return 0, false
	}
	return int64(f), true
}

func intOr(raw any, path ...string) int64 {
	v, _ := intAt(raw, path...)
	return v
}

func optInt(raw any, path ...string) *int64 {
	if v, ok := intAt(raw, path...); ok {
		return &v
	}
	return nil
}

// argumentsObject accepts both the object form used by Ollama and the JSON
// string form used by every other provider.
func argumentsObject(raw any) map[string]any {
	switch v := raw.(type) {
	case map[string]any:
		return v
	case string:
		var out map[string]any
		if err := json.Unmarshal([]byte(v), &out); err == nil && out != nil {
			return out
		}
	}
	return map[string]any{}
}

func argumentsString(args map[string]any) string {
	if args == nil {
		return "{}"
	}
	data, err := json.Marshal(args)
	if err != nil {
		return "{}"
	}
	return string(data)
}

func toOpenAIChat(proto native.Protocol, req Request) (map[string]any, error) {
	if req.Thinking != nil && req.Thinking.BudgetTokens > 0 {
		return nil, unsupported(proto, "budget_tokens")
	}
	messages := make([]map[string]any, 0, len(req.Messages))
	for _, m := range req.Messages {
		var text strings.Builder
		var multi []map[string]any
		hasNonText := false
		var toolCalls []map[string]any
		// OpenAI Chat carries one tool result per message: every ToolResult block
		// becomes its own role=tool message. Folding them into the surrounding
		// turn would drop every call id but the last.
		var toolResults [][2]string
		for _, b := range m.Content {
			switch b.Type {
			case "text":
				text.WriteString(b.Text)
				multi = append(multi, map[string]any{"type": "text", "text": b.Text})
			case "image_url":
				hasNonText = true
				multi = append(multi, map[string]any{"type": "image_url", "image_url": map[string]any{"url": b.URL}})
			case "image_base64":
				hasNonText = true
				multi = append(multi, map[string]any{"type": "image_url", "image_url": map[string]any{"url": "data:" + b.MediaType + ";base64," + b.Data}})
			case "tool_call":
				toolCalls = append(toolCalls, map[string]any{
					"id": b.ID, "type": "function",
					"function": map[string]any{"name": b.Name, "arguments": argumentsString(b.Arguments)},
				})
			case "tool_result":
				toolResults = append(toolResults, [2]string{b.ToolCallID, b.Content})
			case "audio":
				return nil, unsupported(proto, "audio_block")
			}
		}
		if text.Len() > 0 || hasNonText || len(toolCalls) > 0 {
			entry := map[string]any{"role": m.Role}
			if hasNonText {
				entry["content"] = multi
			} else {
				entry["content"] = text.String()
			}
			if len(toolCalls) > 0 {
				entry["tool_calls"] = toolCalls
			}
			messages = append(messages, entry)
		}
		for _, result := range toolResults {
			messages = append(messages, map[string]any{"role": "tool", "tool_call_id": result[0], "content": result[1]})
		}
	}
	out := map[string]any{"model": req.Model, "messages": messages}
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
		out["tools"] = openAITools(req.Tools)
	}
	if choice := openAIToolChoice(req.ToolChoice); choice != nil {
		out["tool_choice"] = choice
	}
	if rf := openAIResponseFormat(req.ResponseFormat); rf != nil {
		out["response_format"] = rf
	}
	return out, nil
}

func openAITools(tools []ToolDefinition) []map[string]any {
	declarations := make([]map[string]any, 0, len(tools))
	for _, t := range tools {
		declarations = append(declarations, map[string]any{
			"type":     "function",
			"function": map[string]any{"name": t.Name, "description": t.Description, "parameters": t.Parameters},
		})
	}
	return declarations
}

func openAIToolChoice(choice *ToolChoice) any {
	if choice == nil {
		return nil
	}
	switch choice.Kind {
	case "auto":
		return "auto"
	case "none":
		return "none"
	case "required":
		return "required"
	default:
		return map[string]any{"type": "function", "function": map[string]any{"name": choice.Name}}
	}
}

func openAIResponseFormat(rf *ResponseFormat) any {
	if rf == nil {
		return nil
	}
	switch rf.Type {
	case "text":
		return map[string]any{"type": "text"}
	case "json_object":
		return map[string]any{"type": "json_object"}
	case "json_schema":
		return map[string]any{"type": "json_schema", "json_schema": map[string]any{
			"name": rf.Name, "schema": rf.Schema, "strict": strictValue(rf.Strict),
		}}
	}
	return nil
}

func strictValue(strict *bool) bool {
	if strict == nil {
		return true
	}
	return *strict
}

func toOpenAIResponses(req Request) (map[string]any, error) {
	if req.Thinking != nil && req.Thinking.BudgetTokens > 0 {
		return nil, unsupported(native.ProtocolOpenAIResponses, "budget_tokens")
	}
	input := make([]map[string]any, 0, len(req.Messages))
	for _, m := range req.Messages {
		role := m.Role
		if role == "tool" {
			// Responses replays tool output as function_call_output items, which live
			// in the user turn rather than a distinct tool role.
			role = "user"
		}
		// Reasoning and function_call items must be emitted in their original
		// order relative to the surrounding text, or a stateless replay loses
		// the reasoning state and the call it produced.
		var parts []map[string]any
		flush := func() {
			if len(parts) > 0 {
				input = append(input, map[string]any{"role": role, "content": parts})
				parts = nil
			}
		}
		for _, b := range m.Content {
			switch b.Type {
			case "text":
				parts = append(parts, map[string]any{"type": "input_text", "text": b.Text})
			case "image_url":
				parts = append(parts, map[string]any{"type": "input_image", "image_url": b.URL})
			case "image_base64":
				parts = append(parts, map[string]any{"type": "input_image", "image_url": "data:" + b.MediaType + ";base64," + b.Data})
			case "thinking":
				flush()
				// `signature` carries the opaque encrypted_content token so a
				// store=false replay keeps working reasoning state.
				item := map[string]any{
					"type":    "reasoning",
					"summary": []map[string]any{{"type": "summary_text", "text": b.Thinking}},
				}
				if b.Signature != "" {
					item["encrypted_content"] = b.Signature
				}
				input = append(input, item)
			case "tool_call":
				flush()
				input = append(input, map[string]any{
					"type": "function_call", "call_id": b.ID, "name": b.Name,
					"arguments": argumentsString(b.Arguments),
				})
			case "tool_result":
				input = append(input, map[string]any{
					"type": "function_call_output", "call_id": b.ToolCallID, "output": b.Content,
				})
			case "audio":
				return nil, unsupported(native.ProtocolOpenAIResponses, "audio_block")
			}
		}
		flush()
	}
	out := map[string]any{"model": req.Model, "input": input}
	if req.MaxTokens > 0 {
		out["max_output_tokens"] = req.MaxTokens
	}
	if req.Thinking != nil && req.Thinking.Effort != "" {
		out["reasoning"] = map[string]any{"effort": req.Thinking.Effort}
	}
	if req.ResponseFormat != nil {
		switch req.ResponseFormat.Type {
		case "text":
		case "json_object":
			out["text"] = map[string]any{"format": map[string]any{"type": "json_object"}}
		case "json_schema":
			out["text"] = map[string]any{"format": map[string]any{
				"type": "json_schema", "name": req.ResponseFormat.Name,
				"schema": req.ResponseFormat.Schema, "strict": strictValue(req.ResponseFormat.Strict),
			}}
		}
	}
	return out, nil
}

func toAnthropicMessages(req Request) (map[string]any, error) {
	if req.ResponseFormat != nil && (req.ResponseFormat.Type == "json_schema" || req.ResponseFormat.Type == "json_object") {
		return nil, unsupported(native.ProtocolAnthropicMessages, "json_schema")
	}
	// Anthropic rejects forced tool use (`any` / `tool`) together with manual
	// extended thinking; report it here instead of forwarding a request the
	// provider answers with an opaque 400.
	forcedChoice := req.ToolChoice != nil && (req.ToolChoice.Kind == "required" || req.ToolChoice.Kind == "tool")
	hasBudget := req.Thinking != nil && req.Thinking.BudgetTokens > 0
	if hasBudget && forcedChoice {
		return nil, unsupported(native.ProtocolAnthropicMessages, "forced_tool_choice_with_thinking")
	}
	var system []string
	messages := make([]map[string]any, 0, len(req.Messages))
	for _, m := range req.Messages {
		if m.Role == "system" {
			for _, b := range m.Content {
				if b.Type == "text" {
					system = append(system, b.Text)
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
				blocks = append(blocks, map[string]any{"type": "text", "text": b.Text})
			case "image_base64":
				blocks = append(blocks, map[string]any{"type": "image", "source": map[string]any{
					"type": "base64", "media_type": b.MediaType, "data": b.Data,
				}})
			case "image_url":
				blocks = append(blocks, map[string]any{"type": "image", "source": map[string]any{
					"type": "url", "url": b.URL,
				}})
			case "thinking":
				block := map[string]any{"type": "thinking", "thinking": b.Thinking}
				if b.Signature != "" {
					block["signature"] = b.Signature
				}
				blocks = append(blocks, block)
			case "tool_call":
				blocks = append(blocks, map[string]any{"type": "tool_use", "id": b.ID, "name": b.Name, "input": b.Arguments})
			case "tool_result":
				blocks = append(blocks, map[string]any{
					"type": "tool_result", "tool_use_id": b.ToolCallID,
					"content": b.Content, "is_error": b.IsError,
				})
			case "audio":
				return nil, unsupported(native.ProtocolAnthropicMessages, "audio_block")
			}
		}
		// Anthropic expects every tool_result for one assistant turn inside a
		// single user message; consecutive same-role turns are merged so a caller
		// that emits one LMP tool message per result still gets one.
		if n := len(messages); n > 0 && messages[n-1]["role"] == role {
			if existing, ok := messages[n-1]["content"].([]map[string]any); ok {
				messages[n-1]["content"] = append(existing, blocks...)
				continue
			}
		}
		messages = append(messages, map[string]any{"role": role, "content": blocks})
	}
	maxTokens := req.MaxTokens
	if maxTokens <= 0 {
		maxTokens = 4096
	}
	out := map[string]any{"model": req.Model, "max_tokens": maxTokens, "messages": messages}
	if len(system) > 0 {
		out["system"] = strings.Join(system, "\n")
	}
	if req.Temperature != nil {
		out["temperature"] = *req.Temperature
	}
	if hasBudget {
		out["thinking"] = map[string]any{"type": "enabled", "budget_tokens": req.Thinking.BudgetTokens}
	}
	// Anthropic steers thinking depth through output_config.effort on models
	// that support it; there is no per-request budget equivalent for effort.
	if req.Thinking != nil && req.Thinking.Effort != "" {
		out["output_config"] = map[string]any{"effort": req.Thinking.Effort}
	}
	if len(req.Tools) > 0 {
		tools := make([]map[string]any, 0, len(req.Tools))
		for _, t := range req.Tools {
			tools = append(tools, map[string]any{"name": t.Name, "description": t.Description, "input_schema": t.Parameters})
		}
		out["tools"] = tools
	}
	if req.ToolChoice != nil {
		switch req.ToolChoice.Kind {
		case "auto":
			out["tool_choice"] = map[string]any{"type": "auto"}
		case "none":
			out["tool_choice"] = map[string]any{"type": "none"}
		case "required":
			out["tool_choice"] = map[string]any{"type": "any"}
		default:
			out["tool_choice"] = map[string]any{"type": "tool", "name": req.ToolChoice.Name}
		}
	}
	return out, nil
}

func toGoogleGemini(proto native.Protocol, req Request) (map[string]any, error) {
	unsupportedFeature := func(feature string) (map[string]any, error) {
		return nil, unsupported(proto, feature)
	}
	contents := make([]map[string]any, 0, len(req.Messages))
	var system []map[string]any
	type callRef struct {
		name   string
		wireID bool
	}
	calls := make(map[string]callRef)
	previousRole := ""
	for _, message := range req.Messages {
		if message.Role == "system" {
			for _, block := range message.Content {
				if block.Type != "text" || block.Signature != "" {
					return unsupportedFeature("system_content")
				}
				system = append(system, map[string]any{"text": block.Text})
			}
			continue
		}
		role := "user"
		if message.Role == "assistant" {
			clear(calls)
			role = "model"
		}
		parts := make([]map[string]any, 0, len(message.Content))
		for _, block := range message.Content {
			var part map[string]any
			var signature string
			switch block.Type {
			case "text":
				part = map[string]any{"text": block.Text}
				signature = block.Signature
			case "thinking":
				part = map[string]any{"thought": true, "text": block.Thinking}
				signature = block.Signature
			case "image_base64", "audio":
				part = map[string]any{"inlineData": map[string]any{"mimeType": block.MediaType, "data": block.Data}}
			case "image_url":
				// Gemini needs a mimeType for remote media, which LMP does not carry.
				return unsupportedFeature("image_url")
			case "tool_call":
				if _, exists := calls[block.ID]; exists {
					return unsupportedFeature("duplicate_tool_call_id")
				}
				wireID := block.ID != "" && block.ID != block.Name && !strings.HasPrefix(block.ID, "gemini-part:")
				calls[block.ID] = callRef{block.Name, wireID}
				call := map[string]any{"name": block.Name, "args": block.Arguments}
				if wireID {
					call["id"] = block.ID
				}
				part = map[string]any{"functionCall": call}
				signature = block.Signature
			case "tool_result":
				ref, ok := calls[block.ToolCallID]
				if !ok {
					return unsupportedFeature("unmatched_tool_result")
				}
				key := "content"
				if block.IsError {
					key = "error"
				}
				response := map[string]any{"name": ref.name, "response": map[string]any{key: block.Content}}
				if ref.wireID {
					response["id"] = block.ToolCallID
				}
				part = map[string]any{"functionResponse": response}
			default:
				return unsupportedFeature(block.Type)
			}
			if signature != "" {
				part["thoughtSignature"] = signature
			}
			parts = append(parts, part)
		}
		if message.Role == "tool" && previousRole == "tool" {
			last := contents[len(contents)-1]
			last["parts"] = append(last["parts"].([]map[string]any), parts...)
		} else {
			contents = append(contents, map[string]any{"role": role, "parts": parts})
		}
		previousRole = message.Role
	}
	body := map[string]any{"contents": contents}
	if len(system) != 0 {
		body["systemInstruction"] = map[string]any{"parts": system}
	}
	generation := map[string]any{}
	if req.MaxTokens > 0 {
		generation["maxOutputTokens"] = req.MaxTokens
	}
	if req.Temperature != nil {
		generation["temperature"] = *req.Temperature
	}
	if req.ResponseFormat != nil {
		switch req.ResponseFormat.Type {
		case "text":
		case "json_object":
			generation["responseMimeType"] = "application/json"
		case "json_schema":
			generation["responseMimeType"] = "application/json"
			generation["responseSchema"] = req.ResponseFormat.Schema
		default:
			return unsupportedFeature("response_format")
		}
	}
	if req.Thinking != nil {
		effort, budget := req.Thinking.Effort, req.Thinking.BudgetTokens
		switch {
		case effort != "" && budget != 0:
			return unsupportedFeature("thinking_budget_and_effort")
		case budget != 0:
			generation["thinkingConfig"] = map[string]any{"thinkingBudget": budget, "includeThoughts": true}
		case effort == "minimal" || effort == "low" || effort == "medium" || effort == "high":
			generation["thinkingConfig"] = map[string]any{"thinkingLevel": strings.ToUpper(effort), "includeThoughts": true}
		case effort != "":
			return unsupportedFeature("thinking_effort")
		}
	}
	if len(generation) != 0 {
		body["generationConfig"] = generation
	}
	if len(req.Tools) != 0 {
		declarations := make([]map[string]any, 0, len(req.Tools))
		for _, tool := range req.Tools {
			declarations = append(declarations, map[string]any{"name": tool.Name, "description": tool.Description, "parameters": tool.Parameters})
		}
		body["tools"] = []map[string]any{{"functionDeclarations": declarations}}
	}
	if req.ToolChoice != nil {
		if len(req.Tools) == 0 {
			return unsupportedFeature("tool_choice_without_tools")
		}
		// Gemini forces a call through toolConfig; `any` can be narrowed to a
		// single function via allowedFunctionNames.
		config := map[string]any{}
		switch req.ToolChoice.Kind {
		case "auto":
			config["mode"] = "AUTO"
		case "none":
			config["mode"] = "NONE"
		case "required":
			config["mode"] = "ANY"
		default:
			config["mode"] = "ANY"
			config["allowedFunctionNames"] = []string{req.ToolChoice.Name}
		}
		body["toolConfig"] = map[string]any{"functionCallingConfig": config}
	}
	return body, nil
}

func toAWSBedrock(req Request) (map[string]any, error) {
	var system []map[string]any
	messages := make([]map[string]any, 0, len(req.Messages))
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
		blocks := make([]map[string]any, 0, len(m.Content))
		for _, b := range m.Content {
			switch b.Type {
			case "text":
				blocks = append(blocks, map[string]any{"text": b.Text})
			case "thinking":
				reasoning := map[string]any{"text": b.Thinking}
				if b.Signature != "" {
					reasoning["signature"] = b.Signature
				}
				blocks = append(blocks, map[string]any{"reasoningContent": map[string]any{"reasoningText": reasoning}})
			case "tool_call":
				blocks = append(blocks, map[string]any{"toolUse": map[string]any{"toolUseId": b.ID, "name": b.Name, "input": b.Arguments}})
			case "tool_result":
				status := "success"
				if b.IsError {
					status = "error"
				}
				blocks = append(blocks, map[string]any{"toolResult": map[string]any{
					"toolUseId": b.ToolCallID, "content": []map[string]any{{"text": b.Content}}, "status": status,
				}})
			case "image_base64":
				format, ok := bedrockImageFormat(b.MediaType)
				if !ok {
					return nil, unsupported(native.ProtocolAWSBedrock, "image_media_type:"+b.MediaType)
				}
				blocks = append(blocks, map[string]any{"image": map[string]any{"format": format, "source": map[string]any{"bytes": b.Data}}})
			case "image_url":
				// Bedrock accepts inline bytes or an S3 location, never a bare URL.
				return nil, unsupported(native.ProtocolAWSBedrock, "image_url")
			case "audio":
				return nil, unsupported(native.ProtocolAWSBedrock, "audio_input")
			}
		}
		messages = append(messages, map[string]any{"role": role, "content": blocks})
	}
	body := map[string]any{"messages": messages}
	if len(system) > 0 {
		body["system"] = system
	}
	if req.MaxTokens > 0 || req.Temperature != nil {
		inference := map[string]any{}
		if req.MaxTokens > 0 {
			inference["maxTokens"] = req.MaxTokens
		}
		if req.Temperature != nil {
			inference["temperature"] = *req.Temperature
		}
		body["inferenceConfig"] = inference
	}
	if len(req.Tools) > 0 {
		tools := make([]map[string]any, 0, len(req.Tools))
		for _, t := range req.Tools {
			tools = append(tools, map[string]any{"toolSpec": map[string]any{
				"name": t.Name, "description": t.Description,
				"inputSchema": map[string]any{"json": t.Parameters},
			}})
		}
		config := map[string]any{"tools": tools}
		if req.ToolChoice != nil {
			// Bedrock models decide whether they honour a forced choice; the wire
			// shape is a one-member union. `none` has no Bedrock equivalent, so it
			// must fail loudly rather than silently degrade.
			switch req.ToolChoice.Kind {
			case "auto":
				config["toolChoice"] = map[string]any{"auto": map[string]any{}}
			case "required":
				config["toolChoice"] = map[string]any{"any": map[string]any{}}
			case "none":
				return nil, unsupported(native.ProtocolAWSBedrock, "tool_choice_none")
			default:
				config["toolChoice"] = map[string]any{"tool": map[string]any{"name": req.ToolChoice.Name}}
			}
		}
		body["toolConfig"] = config
	}
	if req.Thinking != nil && req.Thinking.BudgetTokens > 0 {
		body["additionalModelRequestFields"] = map[string]any{"thinking": map[string]any{
			"type": "enabled", "budget_tokens": req.Thinking.BudgetTokens,
		}}
	}
	return body, nil
}

// bedrockImageFormat maps an image media type to the Bedrock image format enum.
func bedrockImageFormat(mediaType string) (string, bool) {
	switch mediaType {
	case "image/png":
		return "png", true
	case "image/jpeg":
		return "jpeg", true
	case "image/gif":
		return "gif", true
	case "image/webp":
		return "webp", true
	}
	return "", false
}

func toOllamaChat(req Request) (map[string]any, error) {
	if req.ResponseFormat != nil && req.ResponseFormat.Type == "json_schema" {
		return nil, unsupported(native.ProtocolOllama, "json_schema_strict")
	}
	// Ollama has no tool_choice field: `auto` is the default, `none` is
	// expressed by withholding declarations, and forcing a specific call is
	// not representable at all.
	declareTools := len(req.Tools) > 0
	if req.ToolChoice != nil {
		switch req.ToolChoice.Kind {
		case "auto":
		case "none":
			declareTools = false
		default:
			return nil, unsupported(native.ProtocolOllama, "tool_choice")
		}
	}
	// Ollama correlates a tool result with its call through the function name,
	// so remember which call produced each id while replaying the turn.
	callNames := map[string]string{}
	messages := make([]map[string]any, 0, len(req.Messages))
	for _, m := range req.Messages {
		var text strings.Builder
		var thinking strings.Builder
		var images []string
		var toolCalls []map[string]any
		var toolResults [][2]string
		for _, b := range m.Content {
			switch b.Type {
			case "text":
				text.WriteString(b.Text)
			case "thinking":
				thinking.WriteString(b.Thinking)
			case "image_base64":
				images = append(images, b.Data)
			case "image_url":
				images = append(images, b.URL)
			case "tool_call":
				callNames[b.ID] = b.Name
				// Ollama expects arguments as a JSON object, not a string.
				toolCalls = append(toolCalls, map[string]any{"type": "function", "function": map[string]any{
					"index": len(toolCalls), "name": b.Name, "arguments": b.Arguments,
				}})
			case "tool_result":
				toolResults = append(toolResults, [2]string{callNames[b.ToolCallID], b.Content})
			case "audio":
				return nil, unsupported(native.ProtocolOllama, "audio_input")
			}
		}
		if text.Len() > 0 || thinking.Len() > 0 || len(images) > 0 || len(toolCalls) > 0 {
			entry := map[string]any{"role": m.Role, "content": text.String()}
			if thinking.Len() > 0 {
				entry["thinking"] = thinking.String()
			}
			if len(images) > 0 {
				entry["images"] = images
			}
			if len(toolCalls) > 0 {
				entry["tool_calls"] = toolCalls
			}
			messages = append(messages, entry)
		}
		for _, result := range toolResults {
			entry := map[string]any{"role": "tool", "content": result[1]}
			if result[0] != "" {
				entry["tool_name"] = result[0]
			}
			messages = append(messages, entry)
		}
	}
	body := map[string]any{"model": req.Model, "messages": messages, "stream": false}
	if req.ResponseFormat != nil && req.ResponseFormat.Type == "json_object" {
		body["format"] = "json"
	}
	if declareTools {
		body["tools"] = openAITools(req.Tools)
	}
	// `think` accepts a boolean or one of the model's advertised level names.
	if req.Thinking != nil {
		switch req.Thinking.Effort {
		case "minimal", "low", "medium", "high":
			body["think"] = req.Thinking.Effort
		case "":
			body["think"] = true
		default:
			return nil, unsupported(native.ProtocolOllama, "thinking_effort")
		}
	}
	options := map[string]any{}
	if req.Temperature != nil {
		options["temperature"] = *req.Temperature
	}
	if req.MaxTokens > 0 {
		options["num_predict"] = req.MaxTokens
	}
	if len(options) > 0 {
		body["options"] = options
	}
	return body, nil
}

func toCohereChat(req Request) (map[string]any, error) {
	messages := make([]map[string]any, 0, len(req.Messages))
	for _, m := range req.Messages {
		var text strings.Builder
		var thinking strings.Builder
		var images []map[string]any
		var toolCalls []map[string]any
		var toolResults [][2]string
		for _, b := range m.Content {
			switch b.Type {
			case "text":
				text.WriteString(b.Text)
			case "thinking":
				thinking.WriteString(b.Thinking)
			case "image_url":
				images = append(images, map[string]any{"type": "image_url", "image_url": map[string]any{"url": b.URL}})
			case "image_base64":
				return nil, unsupported(native.ProtocolCohere, "image_base64")
			case "tool_call":
				toolCalls = append(toolCalls, map[string]any{"id": b.ID, "type": "function", "function": map[string]any{
					"name": b.Name, "arguments": argumentsString(b.Arguments),
				}})
			case "tool_result":
				toolResults = append(toolResults, [2]string{b.ToolCallID, b.Content})
			case "audio":
				return nil, unsupported(native.ProtocolCohere, "audio_input")
			}
		}
		if m.Role == "assistant" {
			// Cohere replays an assistant turn as an ordered thinking/text block
			// array plus a separate tool_calls field.
			entry := map[string]any{"role": "assistant"}
			var blocks []map[string]any
			if thinking.Len() > 0 {
				blocks = append(blocks, map[string]any{"type": "thinking", "thinking": thinking.String()})
			}
			if text.Len() > 0 {
				blocks = append(blocks, map[string]any{"type": "text", "text": text.String()})
			}
			if len(blocks) > 0 {
				entry["content"] = blocks
			}
			if len(toolCalls) > 0 {
				entry["tool_calls"] = toolCalls
			}
			messages = append(messages, entry)
		} else if len(images) == 0 {
			if text.Len() > 0 {
				messages = append(messages, map[string]any{"role": m.Role, "content": text.String()})
			}
		} else {
			var blocks []map[string]any
			if text.Len() > 0 {
				blocks = append(blocks, map[string]any{"type": "text", "text": text.String()})
			}
			blocks = append(blocks, images...)
			messages = append(messages, map[string]any{"role": m.Role, "content": blocks})
		}
		for _, result := range toolResults {
			messages = append(messages, map[string]any{
				"role": "tool", "tool_call_id": result[0],
				"content": []map[string]any{{"type": "document", "document": map[string]any{"data": result[1]}}},
			})
		}
	}
	body := map[string]any{"model": req.Model, "messages": messages}
	if req.MaxTokens > 0 {
		body["max_tokens"] = req.MaxTokens
	}
	if req.Temperature != nil {
		body["temperature"] = *req.Temperature
	}
	if len(req.Tools) > 0 {
		body["tools"] = openAITools(req.Tools)
	}
	if req.ToolChoice != nil {
		// Cohere only models a binary switch (REQUIRED / NONE). `auto` is its
		// default, so it omits the field instead of sending NONE, which would
		// disable tool calling outright. A named tool has no representation.
		switch req.ToolChoice.Kind {
		case "auto":
		case "none":
			body["tool_choice"] = "NONE"
		case "required":
			body["tool_choice"] = "REQUIRED"
		default:
			return nil, unsupported(native.ProtocolCohere, "tool_choice_tool")
		}
		if _, ok := body["tool_choice"]; ok && len(req.Tools) == 0 {
			return nil, unsupported(native.ProtocolCohere, "tool_choice_without_tools")
		}
	}
	if rf := req.ResponseFormat; rf != nil {
		switch rf.Type {
		case "text":
		case "json_object":
			body["response_format"] = map[string]any{"type": "json_object"}
		case "json_schema":
			body["response_format"] = map[string]any{"type": "json_object", "json_schema": rf.Schema}
		}
	}
	if req.Thinking != nil {
		thinking := map[string]any{"type": "enabled"}
		if req.Thinking.BudgetTokens > 0 {
			thinking["token_budget"] = req.Thinking.BudgetTokens
		}
		body["thinking"] = thinking
	}
	return body, nil
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
		return nil, unsupported(proto, "protocol")
	}
}

func fromOpenAIChat(raw map[string]any) (*Response, error) {
	choices := arrAt(raw, "choices")
	if len(choices) == 0 {
		return nil, fmt.Errorf("missing choices[0]")
	}
	c0 := objAt(choices[0])
	msg := objAt(c0, "message")
	var content []ContentBlock
	if thinking := strAt(msg, "reasoning_content"); thinking != "" {
		content = append(content, ContentBlock{Type: "thinking", Thinking: thinking})
	}
	if text := strAt(msg, "content"); text != "" {
		content = append(content, ContentBlock{Type: "text", Text: text})
	}
	for _, tc := range arrAt(msg, "tool_calls") {
		call := objAt(tc)
		content = append(content, ContentBlock{
			Type: "tool_call", ID: strAt(call, "id"), Name: strAt(call, "function", "name"),
			Arguments: argumentsObject(lookup(call, "function", "arguments")),
		})
	}
	return &Response{
		ID: strAt(raw, "id"), Model: strAt(raw, "model"), Content: content,
		FinishReason: strAt(c0, "finish_reason"), Usage: parseOpenAIUsage(objAt(raw, "usage")), Raw: raw,
	}, nil
}

func fromOpenAIResponses(raw map[string]any) (*Response, error) {
	var content []ContentBlock
	for _, item := range arrAt(raw, "output") {
		entry := objAt(item)
		switch strAt(entry, "type") {
		case "reasoning":
			var parts []string
			for _, s := range arrAt(entry, "summary") {
				if text := strAt(s, "text"); text != "" {
					parts = append(parts, text)
				}
			}
			text := strings.Join(parts, "\n")
			// encrypted_content is the opaque token that preserves reasoning
			// state across a stateless (store=false) replay.
			signature := strAt(entry, "encrypted_content")
			if text != "" || signature != "" {
				content = append(content, ContentBlock{Type: "thinking", Thinking: text, Signature: signature})
			}
		case "message":
			for _, p := range arrAt(entry, "content") {
				if text := strAt(p, "text"); text != "" {
					content = append(content, ContentBlock{Type: "text", Text: text})
				}
			}
		case "function_call":
			content = append(content, ContentBlock{
				Type: "tool_call", ID: strAt(entry, "call_id"), Name: strAt(entry, "name"),
				Arguments: argumentsObject(lookup(entry, "arguments")),
			})
		}
	}
	u := objAt(raw, "usage")
	outDetails := objAt(u, "output_tokens_details")
	inDetails := objAt(u, "input_tokens_details")
	return &Response{
		ID: strAt(raw, "id"), Model: strAt(raw, "model"), Content: content,
		FinishReason: strAt(raw, "status"),
		Usage: Usage{
			InputTokens: intOr(u, "input_tokens"), OutputTokens: intOr(u, "output_tokens"),
			TotalTokens:     intOr(u, "total_tokens"),
			ReasoningTokens: optInt(outDetails, "reasoning_tokens"),
			CacheReadTokens: optInt(inDetails, "cached_tokens"),
		},
		Raw: raw,
	}, nil
}

func fromAnthropicMessages(raw map[string]any) (*Response, error) {
	var content []ContentBlock
	for _, b := range arrAt(raw, "content") {
		block := objAt(b)
		switch strAt(block, "type") {
		case "thinking":
			content = append(content, ContentBlock{Type: "thinking", Thinking: strAt(block, "thinking"), Signature: strAt(block, "signature")})
		case "text":
			content = append(content, ContentBlock{Type: "text", Text: strAt(block, "text")})
		case "tool_use":
			content = append(content, ContentBlock{Type: "tool_call", ID: strAt(block, "id"), Name: strAt(block, "name"), Arguments: argumentsObject(lookup(block, "input"))})
		}
	}
	u := objAt(raw, "usage")
	inTok, outTok := intOr(u, "input_tokens"), intOr(u, "output_tokens")
	return &Response{
		ID: strAt(raw, "id"), Model: strAt(raw, "model"), Content: content,
		FinishReason: strAt(raw, "stop_reason"),
		Usage: Usage{
			InputTokens: inTok, OutputTokens: outTok, TotalTokens: inTok + outTok,
			CacheReadTokens:  optInt(u, "cache_read_input_tokens"),
			CacheWriteTokens: optInt(u, "cache_creation_input_tokens"),
		},
		Raw: raw,
	}, nil
}

func parseOpenAIUsage(u map[string]any) Usage {
	return Usage{
		InputTokens: intOr(u, "prompt_tokens"), OutputTokens: intOr(u, "completion_tokens"),
		TotalTokens:     intOr(u, "total_tokens"),
		ReasoningTokens: optInt(objAt(u, "completion_tokens_details"), "reasoning_tokens"),
		CacheReadTokens: optInt(objAt(u, "prompt_tokens_details"), "cached_tokens"),
	}
}

func fromGoogleGemini(raw map[string]any) (*Response, error) {
	candidates := arrAt(raw, "candidates")
	if len(candidates) == 0 {
		return nil, fmt.Errorf("missing candidates[0]")
	}
	c0 := objAt(candidates[0])
	var content []ContentBlock
	for index, rawPart := range arrAt(c0, "content", "parts") {
		part := objAt(rawPart)
		signature := strAt(part, "thoughtSignature")
		if text, ok := lookup(part, "text").(string); ok {
			if lookup(part, "thought") == true {
				content = append(content, ContentBlock{Type: "thinking", Thinking: text, Signature: signature})
			} else {
				content = append(content, ContentBlock{Type: "text", Text: text, Signature: signature})
			}
		} else if call := objAt(part, "functionCall"); call != nil {
			name := strAt(call, "name")
			if name == "" {
				return nil, fmt.Errorf("functionCall missing name")
			}
			// Google omits the call id for a single-call turn; LMP still needs a
			// stable local id so the matching tool_result can be replayed.
			id := strAt(call, "id")
			if id == "" {
				id = fmt.Sprintf("gemini-part:%d:%s", index, name)
			}
			content = append(content, ContentBlock{Type: "tool_call", ID: id, Name: name, Arguments: argumentsObject(lookup(call, "args")), Signature: signature})
		} else if signature != "" {
			return nil, fmt.Errorf("Gemini returned a signature on an unsupported part")
		}
	}
	u := objAt(raw, "usageMetadata")
	return &Response{
		Content: content, FinishReason: strAt(c0, "finishReason"),
		Usage: Usage{
			InputTokens: intOr(u, "promptTokenCount"), OutputTokens: intOr(u, "candidatesTokenCount"),
			TotalTokens:     intOr(u, "totalTokenCount"),
			ReasoningTokens: optInt(u, "thoughtsTokenCount"),
			CacheReadTokens: optInt(u, "cachedContentTokenCount"),
		},
		Raw: raw,
	}, nil
}

func fromAWSBedrock(raw map[string]any) (*Response, error) {
	var content []ContentBlock
	for _, b := range arrAt(raw, "output", "message", "content") {
		block := objAt(b)
		if text, _ := lookup(block, "text").(string); text != "" {
			content = append(content, ContentBlock{Type: "text", Text: text})
		} else if reasoning := objAt(block, "reasoningContent", "reasoningText"); reasoning != nil {
			content = append(content, ContentBlock{Type: "thinking", Thinking: strAt(reasoning, "text"), Signature: strAt(reasoning, "signature")})
		} else if tool := objAt(block, "toolUse"); tool != nil {
			content = append(content, ContentBlock{Type: "tool_call", ID: strAt(tool, "toolUseId"), Name: strAt(tool, "name"), Arguments: argumentsObject(lookup(tool, "input"))})
		}
	}
	u := objAt(raw, "usage")
	return &Response{
		Content: content, FinishReason: strAt(raw, "stopReason"),
		Usage: Usage{
			InputTokens: intOr(u, "inputTokens"), OutputTokens: intOr(u, "outputTokens"),
			TotalTokens:      intOr(u, "totalTokens"),
			CacheReadTokens:  optInt(u, "cacheReadInputTokens"),
			CacheWriteTokens: optInt(u, "cacheWriteInputTokens"),
		},
		Raw: raw,
	}, nil
}

func fromOllamaChat(raw map[string]any) (*Response, error) {
	msg := objAt(raw, "message")
	var content []ContentBlock
	if thinking := strAt(msg, "thinking"); thinking != "" {
		content = append(content, ContentBlock{Type: "thinking", Thinking: thinking})
	}
	if text := strAt(msg, "content"); text != "" {
		content = append(content, ContentBlock{Type: "text", Text: text})
	}
	for position, rawCall := range arrAt(msg, "tool_calls") {
		call := objAt(rawCall)
		function := objAt(call, "function")
		name := strAt(function, "name")
		// Ollama correlates tool output by function name, so the synthesized id
		// is only a local handle; keep it stable across normalizations.
		index := position
		if v, ok := intAt(function, "index"); ok {
			index = int(v)
		}
		content = append(content, ContentBlock{
			Type: "tool_call", ID: fmt.Sprintf("ollama-call:%d:%s", index, name), Name: name,
			Arguments: argumentsObject(lookup(function, "arguments")),
		})
	}
	inTok, outTok := intOr(raw, "prompt_eval_count"), intOr(raw, "eval_count")
	return &Response{
		Model: strAt(raw, "model"), Content: content, FinishReason: strAt(raw, "done_reason"),
		Usage: Usage{InputTokens: inTok, OutputTokens: outTok, TotalTokens: inTok + outTok}, Raw: raw,
	}, nil
}

func fromCohereChat(raw map[string]any) (*Response, error) {
	msg := objAt(raw, "message")
	var content []ContentBlock
	for _, b := range arrAt(msg, "content") {
		block := objAt(b)
		switch strAt(block, "type") {
		case "thinking":
			if thinking := strAt(block, "thinking"); thinking != "" {
				content = append(content, ContentBlock{Type: "thinking", Thinking: thinking})
			}
		case "text":
			if text := strAt(block, "text"); text != "" {
				content = append(content, ContentBlock{Type: "text", Text: text})
			}
		}
	}
	for _, rawCall := range arrAt(msg, "tool_calls") {
		call := objAt(rawCall)
		content = append(content, ContentBlock{
			Type: "tool_call", ID: strAt(call, "id"), Name: strAt(call, "function", "name"),
			Arguments: argumentsObject(lookup(call, "function", "arguments")),
		})
	}
	u := objAt(raw, "usage", "tokens")
	inTok, outTok := intOr(u, "input_tokens"), intOr(u, "output_tokens")
	return &Response{
		ID: strAt(raw, "id"), Content: content, FinishReason: strAt(raw, "finish_reason"),
		Usage: Usage{InputTokens: inTok, OutputTokens: outTok, TotalTokens: inTok + outTok}, Raw: raw,
	}, nil
}

// StreamNormalizer converts provider streaming frames into LMP stream events.
//
// It is stateful on purpose: tool call arguments arrive as fragments that must
// be attributed to the right call, thinking signatures may land in a later
// frame than the text they belong to, and message_start/message_end have to be
// emitted exactly once no matter what the upstream framing looks like.
type StreamNormalizer struct {
	protocol     native.Protocol
	started      bool
	ended        bool
	usage        Usage
	finishReason string
	// Anthropic content block index -> (tool call id, tool name).
	anthropicTools map[int]struct{ id, name string }
	// Gemini/Vertex send one part per frame, so the part offset is always 0;
	// a monotonic counter keeps parallel tool calls distinguishable.
	geminiToolCalls int
}

// NewStreamNormalizer starts a normalization session for one stream.
func NewStreamNormalizer(proto native.Protocol) *StreamNormalizer {
	return &StreamNormalizer{protocol: proto, anthropicTools: map[int]struct{ id, name string }{}}
}

// Finished reports whether a terminal message_end has been produced.
func (n *StreamNormalizer) Finished() bool { return n.ended }

// Push normalizes one SSE/NDJSON frame into zero or more LMP stream events.
func (n *StreamNormalizer) Push(ev native.SSEEvent) ([]StreamEvent, error) {
	var out []StreamEvent
	trimmed := strings.TrimSpace(ev.Data)
	if trimmed == "" || trimmed == "[DONE]" {
		return out, nil
	}
	var value map[string]any
	if err := json.Unmarshal([]byte(trimmed), &value); err != nil {
		return nil, err
	}
	switch n.protocol {
	case native.ProtocolOpenAIChatCompletions, native.ProtocolAzureOpenAI:
		n.pushOpenAIChat(value, &out)
	case native.ProtocolOpenAIResponses:
		n.pushOpenAIResponses(ev.Event, value, &out)
	case native.ProtocolAnthropicMessages:
		n.pushAnthropic(ev.Event, value, &out)
	case native.ProtocolGoogleGemini, native.ProtocolGoogleVertexAI:
		n.pushGemini(value, &out)
	case native.ProtocolAWSBedrock:
		n.pushBedrock(value, &out)
	case native.ProtocolOllama:
		n.pushOllama(value, &out)
	case native.ProtocolCohere:
		n.pushCohere(ev.Event, value, &out)
	default:
		return nil, unsupported(n.protocol, "protocol")
	}
	return out, nil
}

// Finish emits message_end for protocols whose upstream stream ends without an
// explicit terminal frame.
func (n *StreamNormalizer) Finish() []StreamEvent {
	if n.ended {
		return nil
	}
	n.ended = true
	return []StreamEvent{{Type: "message_end", FinishReason: n.finishReason}}
}

func (n *StreamNormalizer) markStarted(id string, out *[]StreamEvent) {
	if n.started {
		return
	}
	n.started = true
	*out = append(*out, StreamEvent{Type: "message_start", ID: id})
}

func (n *StreamNormalizer) markEnded(finishReason string, out *[]StreamEvent) {
	if finishReason != "" {
		n.finishReason = finishReason
	}
	if n.ended {
		return
	}
	n.ended = true
	*out = append(*out, StreamEvent{Type: "message_end", FinishReason: finishReason})
}

// emitUsage keeps only changes: Anthropic repeats the same counters in several
// frames and a consumer should see one usage event per distinct value.
func (n *StreamNormalizer) emitUsage(usage Usage, out *[]StreamEvent) {
	changed := !usageEqual(n.usage, usage)
	n.usage = usage
	if changed {
		u := usage
		*out = append(*out, StreamEvent{Type: "usage", Usage: &u})
	}
}

func usageEqual(a, b Usage) bool {
	if a.InputTokens != b.InputTokens || a.OutputTokens != b.OutputTokens || a.TotalTokens != b.TotalTokens {
		return false
	}
	return intPtrEqual(a.ReasoningTokens, b.ReasoningTokens) && intPtrEqual(a.CacheReadTokens, b.CacheReadTokens) && intPtrEqual(a.CacheWriteTokens, b.CacheWriteTokens)
}

func intPtrEqual(a, b *int64) bool {
	if a == nil || b == nil {
		return a == b
	}
	return *a == *b
}

func (n *StreamNormalizer) pushOpenAIChat(value map[string]any, out *[]StreamEvent) {
	if choices := arrAt(value, "choices"); len(choices) > 0 {
		choice := objAt(choices[0])
		if delta := objAt(choice, "delta"); delta != nil {
			if reasoning, _ := lookup(delta, "reasoning_content").(string); reasoning != "" {
				*out = append(*out, StreamEvent{Type: "thinking_delta", Delta: reasoning})
			}
			if text, _ := lookup(delta, "content").(string); text != "" {
				*out = append(*out, StreamEvent{Type: "text_delta", Delta: text})
			}
			for position, rawCall := range arrAt(delta, "tool_calls") {
				call := objAt(rawCall)
				index := position
				if v, ok := intAt(call, "index"); ok {
					index = int(v)
				}
				arguments, _ := lookup(call, "function", "arguments").(string)
				callIndex := index
				*out = append(*out, StreamEvent{
					Type: "tool_call_delta", Index: &callIndex, ID: strAt(call, "id"),
					Name: strAt(call, "function", "name"), ArgumentsDelta: arguments,
				})
			}
		}
		if reason := strAt(choice, "finish_reason"); reason != "" {
			n.markEnded(reason, out)
		}
	}
	if usage := objAt(value, "usage"); usage != nil {
		n.emitUsage(parseOpenAIUsage(usage), out)
	}
}

func (n *StreamNormalizer) pushOpenAIResponses(eventName string, value map[string]any, out *[]StreamEvent) {
	kind := eventName
	if kind == "" {
		kind = strAt(value, "type")
	}
	switch kind {
	case "response.created":
		n.markStarted(strAt(value, "response", "id"), out)
	case "response.output_text.delta":
		if delta := strAt(value, "delta"); delta != "" {
			*out = append(*out, StreamEvent{Type: "text_delta", Delta: delta})
		}
	case "response.reasoning_summary_text.delta", "response.reasoning_text.delta":
		if delta := strAt(value, "delta"); delta != "" {
			*out = append(*out, StreamEvent{Type: "thinking_delta", Delta: delta})
		}
	case "response.output_item.added":
		item := objAt(value, "item")
		if strAt(item, "type") == "function_call" {
			index := int(intOr(value, "output_index"))
			id := strAt(item, "call_id")
			if id == "" {
				id = strAt(item, "id")
			}
			*out = append(*out, StreamEvent{Type: "tool_call_delta", Index: &index, ID: id, Name: strAt(item, "name")})
		}
	case "response.function_call_arguments.delta":
		if delta := strAt(value, "delta"); delta != "" {
			index := int(intOr(value, "output_index"))
			*out = append(*out, StreamEvent{Type: "tool_call_delta", Index: &index, ArgumentsDelta: delta})
		}
	case "response.completed":
		response := objAt(value, "response")
		if usage := objAt(response, "usage"); usage != nil {
			inputTokens, outputTokens := intOr(usage, "input_tokens"), intOr(usage, "output_tokens")
			total := intOr(usage, "total_tokens")
			if total == 0 {
				total = inputTokens + outputTokens
			}
			n.emitUsage(Usage{
				InputTokens: inputTokens, OutputTokens: outputTokens, TotalTokens: total,
				ReasoningTokens: optInt(objAt(usage, "output_tokens_details"), "reasoning_tokens"),
				CacheReadTokens: optInt(objAt(usage, "input_tokens_details"), "cached_tokens"),
			}, out)
		}
		n.markEnded(strAt(response, "status"), out)
	}
}

func (n *StreamNormalizer) pushAnthropic(eventName string, value map[string]any, out *[]StreamEvent) {
	kind := eventName
	if kind == "" {
		kind = strAt(value, "type")
	}
	switch kind {
	case "message_start":
		message := objAt(value, "message")
		n.markStarted(strAt(message, "id"), out)
		if usage := objAt(message, "usage"); usage != nil {
			n.emitUsage(parseAnthropicUsage(usage, n.usage), out)
		}
	case "content_block_start":
		index := int(intOr(value, "index"))
		block := objAt(value, "content_block")
		switch strAt(block, "type") {
		case "tool_use":
			id, name := strAt(block, "id"), strAt(block, "name")
			n.anthropicTools[index] = struct{ id, name string }{id, name}
			blockIndex := index
			*out = append(*out, StreamEvent{Type: "tool_call_delta", Index: &blockIndex, ID: id, Name: name})
		case "thinking":
			// The signature may be announced up front; forward it as its own
			// delta so a replay can attach it to the reasoning text.
			if signature := strAt(block, "signature"); signature != "" {
				*out = append(*out, StreamEvent{Type: "thinking_delta", Signature: signature})
			}
		}
	case "content_block_delta":
		index := int(intOr(value, "index"))
		delta := objAt(value, "delta")
		switch strAt(delta, "type") {
		case "thinking_delta":
			if text := strAt(delta, "thinking"); text != "" {
				*out = append(*out, StreamEvent{Type: "thinking_delta", Delta: text})
			}
		case "text_delta":
			if text := strAt(delta, "text"); text != "" {
				*out = append(*out, StreamEvent{Type: "text_delta", Delta: text})
			}
		case "input_json_delta":
			if partial := strAt(delta, "partial_json"); partial != "" {
				known := n.anthropicTools[index]
				blockIndex := index
				*out = append(*out, StreamEvent{
					Type: "tool_call_delta", Index: &blockIndex, ID: known.id, Name: known.name,
					ArgumentsDelta: partial,
				})
			}
		case "signature_delta":
			if signature := strAt(delta, "signature"); signature != "" {
				*out = append(*out, StreamEvent{Type: "thinking_delta", Signature: signature})
			}
		}
	case "message_delta":
		if usage := objAt(value, "usage"); usage != nil {
			n.emitUsage(parseAnthropicUsage(usage, n.usage), out)
		}
		// message_delta announces the stop reason; the terminal message_end is
		// only emitted on message_stop.
		if reason := strAt(value, "delta", "stop_reason"); reason != "" {
			n.finishReason = reason
		}
	case "message_stop":
		n.markEnded(n.finishReason, out)
	}
}

// parseAnthropicUsage merges cache counters with the previous totals, because
// stream deltas only carry the changed counters.
func parseAnthropicUsage(usage map[string]any, previous Usage) Usage {
	inputTokens, inputKnown := intAt(usage, "input_tokens")
	if !inputKnown {
		inputTokens = previous.InputTokens
	}
	outputTokens, outputKnown := intAt(usage, "output_tokens")
	if !outputKnown {
		outputTokens = previous.OutputTokens
	}
	total, totalKnown := intAt(usage, "total_tokens")
	if !totalKnown {
		total = inputTokens + outputTokens
	}
	parsed := Usage{InputTokens: inputTokens, OutputTokens: outputTokens, TotalTokens: total}
	if v := optInt(usage, "cache_read_input_tokens"); v != nil {
		parsed.CacheReadTokens = v
	} else {
		parsed.CacheReadTokens = previous.CacheReadTokens
	}
	if v := optInt(usage, "cache_creation_input_tokens"); v != nil {
		parsed.CacheWriteTokens = v
	} else {
		parsed.CacheWriteTokens = previous.CacheWriteTokens
	}
	return parsed
}

func (n *StreamNormalizer) pushGemini(value map[string]any, out *[]StreamEvent) {
	if candidates := arrAt(value, "candidates"); len(candidates) > 0 {
		candidate := objAt(candidates[0])
		for _, rawPart := range arrAt(candidate, "content", "parts") {
			part := objAt(rawPart)
			signature := strAt(part, "thoughtSignature")
			if text, ok := lookup(part, "text").(string); ok {
				if lookup(part, "thought") == true {
					*out = append(*out, StreamEvent{Type: "thinking_delta", Delta: text, Signature: signature})
				} else {
					*out = append(*out, StreamEvent{Type: "text_delta", Delta: text, Signature: signature})
				}
			} else if call := objAt(part, "functionCall"); call != nil {
				// Each frame carries one part, so the part offset cannot identify
				// parallel calls; a monotonic counter can.
				index := n.geminiToolCalls
				n.geminiToolCalls++
				arguments := ""
				if args := lookup(call, "args"); args != nil {
					if encoded, err := json.Marshal(args); err == nil {
						arguments = string(encoded)
					}
				}
				*out = append(*out, StreamEvent{
					Type: "tool_call_delta", Index: &index, ID: strAt(call, "id"),
					Name: strAt(call, "name"), ArgumentsDelta: arguments, Signature: signature,
				})
			}
		}
		if reason := strAt(candidate, "finishReason"); reason != "" {
			n.finishReason = reason
		}
	}
	if usage := objAt(value, "usageMetadata"); usage != nil {
		inputTokens, outputTokens := intOr(usage, "promptTokenCount"), intOr(usage, "candidatesTokenCount")
		total := intOr(usage, "totalTokenCount")
		if total == 0 {
			total = inputTokens + outputTokens
		}
		n.emitUsage(Usage{
			InputTokens: inputTokens, OutputTokens: outputTokens, TotalTokens: total,
			ReasoningTokens: optInt(usage, "thoughtsTokenCount"),
			CacheReadTokens: optInt(usage, "cachedContentTokenCount"),
		}, out)
	}
}

func (n *StreamNormalizer) pushBedrock(value map[string]any, out *[]StreamEvent) {
	if delta := objAt(value, "contentBlockDelta"); delta != nil {
		index := int(intOr(delta, "contentBlockIndex"))
		payload := objAt(delta, "delta")
		if text, _ := lookup(payload, "text").(string); text != "" {
			*out = append(*out, StreamEvent{Type: "text_delta", Delta: text})
		}
		if reasoning := objAt(payload, "reasoningContent"); reasoning != nil {
			if text := strAt(reasoning, "text"); text != "" {
				*out = append(*out, StreamEvent{Type: "thinking_delta", Delta: text})
			}
			if signature := strAt(reasoning, "signature"); signature != "" {
				*out = append(*out, StreamEvent{Type: "thinking_delta", Signature: signature})
			}
		}
		if input, ok := lookup(payload, "toolUse", "input").(string); ok && input != "" {
			*out = append(*out, StreamEvent{Type: "tool_call_delta", Index: &index, ArgumentsDelta: input})
		}
	} else if start := objAt(value, "contentBlockStart", "start", "toolUse"); start != nil {
		index := int(intOr(value, "contentBlockStart", "contentBlockIndex"))
		*out = append(*out, StreamEvent{Type: "tool_call_delta", Index: &index, ID: strAt(start, "toolUseId"), Name: strAt(start, "name")})
	} else if reason := strAt(value, "messageStop", "stopReason"); reason != "" {
		n.markEnded(reason, out)
	} else if usage := objAt(value, "metadata", "usage"); usage != nil {
		inputTokens, outputTokens := intOr(usage, "inputTokens"), intOr(usage, "outputTokens")
		total := intOr(usage, "totalTokens")
		if total == 0 {
			total = inputTokens + outputTokens
		}
		n.emitUsage(Usage{
			InputTokens: inputTokens, OutputTokens: outputTokens, TotalTokens: total,
			CacheReadTokens:  optInt(usage, "cacheReadInputTokens"),
			CacheWriteTokens: optInt(usage, "cacheWriteInputTokens"),
		}, out)
	}
}

func (n *StreamNormalizer) pushOllama(value map[string]any, out *[]StreamEvent) {
	message := objAt(value, "message")
	if thinking := strAt(message, "thinking"); thinking != "" {
		*out = append(*out, StreamEvent{Type: "thinking_delta", Delta: thinking})
	}
	if text := strAt(message, "content"); text != "" {
		*out = append(*out, StreamEvent{Type: "text_delta", Delta: text})
	}
	for position, rawCall := range arrAt(message, "tool_calls") {
		function := objAt(objAt(rawCall), "function")
		name := strAt(function, "name")
		index := position
		if v, ok := intAt(function, "index"); ok {
			index = int(v)
		}
		arguments, _ := json.Marshal(lookup(function, "arguments"))
		callIndex := index
		*out = append(*out, StreamEvent{
			Type: "tool_call_delta", Index: &callIndex, ID: fmt.Sprintf("ollama-call:%d:%s", index, name),
			Name: name, ArgumentsDelta: string(arguments),
		})
	}
	if done, _ := lookup(value, "done").(bool); done {
		inputTokens, outputTokens := intOr(value, "prompt_eval_count"), intOr(value, "eval_count")
		n.emitUsage(Usage{InputTokens: inputTokens, OutputTokens: outputTokens, TotalTokens: inputTokens + outputTokens}, out)
		n.markEnded(strAt(value, "done_reason"), out)
	}
}

func (n *StreamNormalizer) pushCohere(eventName string, value map[string]any, out *[]StreamEvent) {
	kind := eventName
	if kind == "" {
		kind = strAt(value, "type")
	}
	switch kind {
	case "message-start":
		n.markStarted(strAt(value, "id"), out)
	case "tool-call-start":
		// Cohere announces each call then streams its arguments in fragments.
		index := int(intOr(value, "index"))
		call := lookup(value, "delta", "message", "tool_calls")
		if calls, ok := call.([]any); ok && len(calls) > 0 {
			call = calls[0]
		}
		*out = append(*out, StreamEvent{Type: "tool_call_delta", Index: &index, ID: strAt(call, "id"), Name: strAt(call, "function", "name")})
	case "tool-call-delta":
		index := int(intOr(value, "index"))
		if delta := strAt(value, "delta", "message", "tool_calls", "function", "arguments"); delta != "" {
			*out = append(*out, StreamEvent{Type: "tool_call_delta", Index: &index, ArgumentsDelta: delta})
		}
	case "content-delta":
		if text := strAt(value, "delta", "message", "content", "text"); text != "" {
			*out = append(*out, StreamEvent{Type: "text_delta", Delta: text})
		}
		if thinking := strAt(value, "delta", "message", "content", "thinking"); thinking != "" {
			*out = append(*out, StreamEvent{Type: "thinking_delta", Delta: thinking})
		}
	case "message-end":
		if tokens := objAt(value, "delta", "usage", "tokens"); tokens != nil {
			inputTokens, outputTokens := intOr(tokens, "input_tokens"), intOr(tokens, "output_tokens")
			n.emitUsage(Usage{InputTokens: inputTokens, OutputTokens: outputTokens, TotalTokens: inputTokens + outputTokens}, out)
		}
		n.markEnded(strAt(value, "delta", "finish_reason"), out)
	}
}

// NormalizeStreamEvents buffers a whole stream through a fresh normalizer.
func NormalizeStreamEvents(proto native.Protocol, events []native.SSEEvent) ([]StreamEvent, error) {
	normalizer := NewStreamNormalizer(proto)
	var out []StreamEvent
	for _, ev := range events {
		produced, err := normalizer.Push(ev)
		if err != nil {
			return nil, err
		}
		out = append(out, produced...)
	}
	out = append(out, normalizer.Finish()...)
	return out, nil
}

// NormalizeStreamEvent normalizes one frame without cross-frame state.
//
// Prefer NewStreamNormalizer: a stateless call cannot attribute a fragmented
// tool argument to its call, dedupe usage, or emit message_start/message_end.
// It stays for callers that only ever see a single complete frame.
func NormalizeStreamEvent(proto native.Protocol, ev native.SSEEvent) ([]StreamEvent, error) {
	return NewStreamNormalizer(proto).Push(ev)
}
