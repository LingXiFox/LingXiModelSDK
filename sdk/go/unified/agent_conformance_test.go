package unified

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/LingXiFox/LingXiModelSDK/sdk/go/native"
)

const agentDir = "../../../conformance/agent"

func loadAgentJSON(t *testing.T, name string) []byte {
	t.Helper()
	b, err := os.ReadFile(filepath.Join(agentDir, name))
	if err != nil {
		t.Fatalf("read %s: %v", name, err)
	}
	return b
}

type agentExpectations struct {
	Protocols []string                       `json:"protocols"`
	Requests  map[string]string              `json:"requests"`
	Responses map[string]agentResponseExpect `json:"responses"`
	Streams   map[string]agentStreamExpect   `json:"streams"`
}

type agentResponseExpect struct {
	ID                *string         `json:"id"`
	Text              string          `json:"text"`
	Thinking          string          `json:"thinking"`
	ThinkingSignature *string         `json:"thinkingSignature"`
	ToolCalls         []agentToolCall `json:"toolCalls"`
	FinishReason      string          `json:"finishReason"`
	Usage             map[string]any  `json:"usage"`
	ContentOrder      []string        `json:"contentOrder"`
}

type agentStreamExpect struct {
	ID           *string         `json:"id"`
	Text         string          `json:"text"`
	Thinking     string          `json:"thinking"`
	ToolCalls    []agentToolCall `json:"toolCalls"`
	Signatures   [][2]string     `json:"signatures"`
	FinishReason string          `json:"finishReason"`
	Usage        map[string]any  `json:"usage"`
}

type agentToolCall struct {
	Index     *int           `json:"index"`
	ID        string         `json:"id"`
	Name      string         `json:"name"`
	Arguments map[string]any `json:"arguments"`
}

func loadExpectations(t *testing.T) agentExpectations {
	t.Helper()
	var exp agentExpectations
	if err := json.Unmarshal(loadAgentJSON(t, "expectations.json"), &exp); err != nil {
		t.Fatalf("decode expectations: %v", err)
	}
	return exp
}

// normalizeNumbers round-trips a decoded JSON document through marshal so
// fixture literals and generated values compare with the same Go types.
func normalizeNumbers(t *testing.T, value any) any {
	t.Helper()
	b, err := json.Marshal(value)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	var out any
	if err := json.Unmarshal(b, &out); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	return out
}

// withoutNulls drops the explicit nulls the Rust core emits for absent
// optional counters; Go omits them instead of keeping a nil entry.
func withoutNulls(t *testing.T, value any) map[string]any {
	t.Helper()
	m, ok := normalizeNumbers(t, value).(map[string]any)
	if !ok {
		t.Fatalf("expected an object, got %T", value)
	}
	for key, entry := range m {
		if entry == nil {
			delete(m, key)
		}
	}
	return m
}

func protocolOf(t *testing.T, name string) native.Protocol {
	t.Helper()
	for _, candidate := range []native.Protocol{
		native.ProtocolOpenAIChatCompletions, native.ProtocolOpenAIResponses,
		native.ProtocolAnthropicMessages, native.ProtocolGoogleGemini,
		native.ProtocolAWSBedrock, native.ProtocolGoogleVertexAI,
		native.ProtocolAzureOpenAI, native.ProtocolOllama, native.ProtocolCohere,
	} {
		if string(candidate) == name {
			return candidate
		}
	}
	t.Fatalf("unknown protocol %q", name)
	return ""
}

func TestAgentFixturesNormalizeToSharedResponses(t *testing.T) {
	exp := loadExpectations(t)
	for _, name := range exp.Protocols {
		t.Run(name, func(t *testing.T) {
			proto := protocolOf(t, name)
			var raw map[string]any
			if err := json.Unmarshal(loadAgentJSON(t, filepath.Join("responses", name+".json")), &raw); err != nil {
				t.Fatalf("decode response fixture: %v", err)
			}
			resp, err := NormalizeResponse(proto, raw)
			if err != nil {
				t.Fatalf("normalize: %v", err)
			}
			expected := exp.Responses[name]
			var text, thinking strings.Builder
			var order []string
			var calls []agentToolCall
			var thinkingSignature *string
			for _, block := range resp.Content {
				switch block.Type {
				case "thinking":
					thinking.WriteString(block.Thinking)
					order = append(order, "thinking")
					if block.Signature != "" {
						sig := block.Signature
						thinkingSignature = &sig
					}
				case "text":
					text.WriteString(block.Text)
					order = append(order, "text")
				case "tool_call":
					order = append(order, "tool_call")
					calls = append(calls, agentToolCall{ID: block.ID, Name: block.Name, Arguments: block.Arguments})
				}
			}
			if text.String() != expected.Text {
				t.Fatalf("text = %q, want %q", text.String(), expected.Text)
			}
			if thinking.String() != expected.Thinking {
				t.Fatalf("thinking = %q, want %q", thinking.String(), expected.Thinking)
			}
			if !reflect.DeepEqual(order, expected.ContentOrder) {
				t.Fatalf("content order = %v, want %v", order, expected.ContentOrder)
			}
			if resp.FinishReason != expected.FinishReason {
				t.Fatalf("finish reason = %q, want %q", resp.FinishReason, expected.FinishReason)
			}
			switch {
			case thinkingSignature == nil && expected.ThinkingSignature == nil:
			case thinkingSignature == nil || expected.ThinkingSignature == nil:
				t.Fatalf("thinking signature = %v, want %v", thinkingSignature, expected.ThinkingSignature)
			case *thinkingSignature != *expected.ThinkingSignature:
				t.Fatalf("thinking signature = %q, want %q", *thinkingSignature, *expected.ThinkingSignature)
			}
			if len(calls) != len(expected.ToolCalls) {
				t.Fatalf("tool calls = %d, want %d", len(calls), len(expected.ToolCalls))
			}
			for i, call := range calls {
				want := expected.ToolCalls[i]
				if call.ID != want.ID || call.Name != want.Name {
					t.Fatalf("call %d = %s/%s, want %s/%s", i, call.ID, call.Name, want.ID, want.Name)
				}
				if !reflect.DeepEqual(normalizeNumbers(t, call.Arguments), normalizeNumbers(t, want.Arguments)) {
					t.Fatalf("call %d arguments = %v, want %v", i, call.Arguments, want.Arguments)
				}
			}
			gotUsage := withoutNulls(t, resp.Usage)
			wantUsage := withoutNulls(t, expected.Usage)
			if !reflect.DeepEqual(gotUsage, wantUsage) {
				t.Fatalf("usage = %v, want %v", gotUsage, wantUsage)
			}
		})
	}
}

type aggregatedStream struct {
	ID           string
	Text         strings.Builder
	Thinking     strings.Builder
	Calls        map[int]*agentToolCall
	Arguments    map[int]*strings.Builder
	Signatures   [][2]string
	FinishReason string
	Usage        *Usage
}

func aggregateStream(events []StreamEvent) (*aggregatedStream, error) {
	out := &aggregatedStream{Calls: map[int]*agentToolCall{}, Arguments: map[int]*strings.Builder{}}
	for _, event := range events {
		switch event.Type {
		case "message_start":
			if out.ID == "" && event.ID != "" {
				out.ID = event.ID
			}
		case "thinking_delta":
			out.Thinking.WriteString(event.Delta)
			if event.Signature != "" {
				out.Signatures = append(out.Signatures, [2]string{"thinking", event.Signature})
			}
		case "text_delta":
			out.Text.WriteString(event.Delta)
			if event.Signature != "" {
				out.Signatures = append(out.Signatures, [2]string{"text", event.Signature})
			}
		case "tool_call_delta":
			index := 0
			if event.Index != nil {
				index = *event.Index
			}
			call, ok := out.Calls[index]
			if !ok {
				call = &agentToolCall{}
				out.Calls[index] = call
				out.Arguments[index] = &strings.Builder{}
			}
			if event.ID != "" {
				call.ID = event.ID
			}
			if event.Name != "" {
				call.Name = event.Name
			}
			out.Arguments[index].WriteString(event.ArgumentsDelta)
			if event.Signature != "" {
				out.Signatures = append(out.Signatures, [2]string{"tool_call", event.Signature})
			}
		case "usage":
			if event.Usage != nil {
				usage := *event.Usage
				out.Usage = &usage
			}
		case "message_end":
			out.FinishReason = event.FinishReason
		case "error":
			return nil, errors.New(event.Code + ": " + event.Message)
		}
	}
	for index, call := range out.Calls {
		serialized := out.Arguments[index].String()
		if serialized == "" {
			call.Arguments = map[string]any{}
			continue
		}
		call.Arguments = argumentsObject(serialized)
	}
	return out, nil
}

func TestAgentFixturesAggregateToSharedStreamedShape(t *testing.T) {
	exp := loadExpectations(t)
	for _, name := range exp.Protocols {
		t.Run(name, func(t *testing.T) {
			proto := protocolOf(t, name)
			var doc struct {
				Events []struct {
					Event *string `json:"event"`
					Data  string  `json:"data"`
					ID    *string `json:"id"`
				} `json:"events"`
			}
			if err := json.Unmarshal(loadAgentJSON(t, filepath.Join("streams", name+".json")), &doc); err != nil {
				t.Fatalf("decode stream fixture: %v", err)
			}
			events := make([]native.SSEEvent, 0, len(doc.Events))
			for _, entry := range doc.Events {
				frame := native.SSEEvent{Data: entry.Data}
				if entry.Event != nil {
					frame.Event = *entry.Event
				}
				if entry.ID != nil {
					frame.ID = *entry.ID
				}
				events = append(events, frame)
			}
			got, err := NormalizeStreamEvents(proto, events)
			if err != nil {
				t.Fatalf("normalize stream: %v", err)
			}
			aggregated, err := aggregateStream(got)
			if err != nil {
				t.Fatalf("aggregate: %v", err)
			}
			expected := exp.Streams[name]
			if aggregated.Text.String() != expected.Text {
				t.Fatalf("text = %q, want %q", aggregated.Text.String(), expected.Text)
			}
			if aggregated.Thinking.String() != expected.Thinking {
				t.Fatalf("thinking = %q, want %q", aggregated.Thinking.String(), expected.Thinking)
			}
			if aggregated.FinishReason != expected.FinishReason {
				t.Fatalf("finish reason = %q, want %q", aggregated.FinishReason, expected.FinishReason)
			}
			// The fixture stores an empty list where Go leaves a nil slice, so
			// compare the contents rather than the slice headers.
			if len(aggregated.Signatures) != len(expected.Signatures) {
				t.Fatalf("signatures = %v, want %v", aggregated.Signatures, expected.Signatures)
			}
			for i, signature := range aggregated.Signatures {
				if signature != expected.Signatures[i] {
					t.Fatalf("signature %d = %v, want %v", i, signature, expected.Signatures[i])
				}
			}
			if len(aggregated.Calls) != len(expected.ToolCalls) {
				t.Fatalf("tool calls = %d, want %d", len(aggregated.Calls), len(expected.ToolCalls))
			}
			for _, want := range expected.ToolCalls {
				index := 0
				if want.Index != nil {
					index = *want.Index
				}
				call, ok := aggregated.Calls[index]
				if !ok {
					t.Fatalf("missing tool call at index %d", index)
				}
				if call.ID != want.ID || call.Name != want.Name {
					t.Fatalf("call %d = %s/%s, want %s/%s", index, call.ID, call.Name, want.ID, want.Name)
				}
				if !reflect.DeepEqual(normalizeNumbers(t, call.Arguments), normalizeNumbers(t, want.Arguments)) {
					t.Fatalf("call %d arguments = %v, want %v", index, call.Arguments, want.Arguments)
				}
			}
			if aggregated.Usage == nil {
				t.Fatalf("stream produced no usage event")
			}
			if !reflect.DeepEqual(withoutNulls(t, *aggregated.Usage), withoutNulls(t, expected.Usage)) {
				t.Fatalf("usage = %v, want %v", *aggregated.Usage, expected.Usage)
			}
		})
	}
}
func TestAgentTranslationsMatchSharedSnapshot(t *testing.T) {
	var snapshot struct {
		Protocols []string `json:"protocols"`
		Requests  map[string]map[string]struct {
			Payload map[string]any `json:"payload"`
			Error   *struct {
				Code    string `json:"code"`
				Message string `json:"message"`
			} `json:"error"`
		} `json:"requests"`
	}
	if err := json.Unmarshal(loadAgentJSON(t, "translations.json"), &snapshot); err != nil {
		t.Fatalf("decode translations: %v", err)
	}
	exp := loadExpectations(t)
	checked := 0
	for requestID, relPath := range exp.Requests {
		t.Run(requestID, func(t *testing.T) {
			var doc struct {
				Request Request `json:"request"`
			}
			if err := json.Unmarshal(loadAgentJSON(t, relPath), &doc); err != nil {
				t.Fatalf("decode request fixture: %v", err)
			}
			for _, name := range snapshot.Protocols {
				proto := protocolOf(t, name)
				want, ok := snapshot.Requests[requestID][name]
				if !ok {
					t.Fatalf("snapshot has no %s/%s entry", requestID, name)
				}
				payload, err := TranslateRequest(proto, doc.Request)
				if want.Error != nil {
					if err == nil {
						t.Fatalf("%s: expected %s, got a payload", name, want.Error.Code)
					}
					var unsupported *UnsupportedFeatureError
					if !errors.As(err, &unsupported) {
						t.Fatalf("%s: expected a typed unsupported error, got %v", name, err)
					}
					if want.Error.Code != "unsupported_feature" {
						t.Fatalf("%s: unexpected snapshot code %s", name, want.Error.Code)
					}
					if !strings.Contains(want.Error.Message, unsupported.Feature) {
						t.Fatalf("%s: message %q does not mention feature %q", name, want.Error.Message, unsupported.Feature)
					}
					checked++
					continue
				}
				if err != nil {
					t.Fatalf("%s: unexpected error: %v", name, err)
				}
				got := normalizeNumbers(t, payload)
				if !reflect.DeepEqual(got, normalizeNumbers(t, want.Payload)) {
					t.Fatalf("%s: payload mismatch\ngot:  %v\nwant: %v", name, got, want.Payload)
				}
				checked++
			}
		})
	}
	if checked != 27 {
		t.Fatalf("checked %d translation cells, want 27", checked)
	}
}

func TestNormalizedAssistantTurnsReplayEveryToolCallAndResult(t *testing.T) {
	exp := loadExpectations(t)
	for _, name := range exp.Protocols {
		t.Run(name, func(t *testing.T) {
			proto := protocolOf(t, name)
			var raw map[string]any
			if err := json.Unmarshal(loadAgentJSON(t, filepath.Join("responses", name+".json")), &raw); err != nil {
				t.Fatalf("decode response fixture: %v", err)
			}
			normalized, err := NormalizeResponse(proto, raw)
			if err != nil {
				t.Fatalf("normalize: %v", err)
			}
			var results []ContentBlock
			for _, block := range normalized.Content {
				if block.Type != "tool_call" {
					continue
				}
				results = append(results, ContentBlock{Type: "tool_result", ToolCallID: block.ID, Content: "ok"})
			}
			request := Request{Model: "mock-1", Messages: []Message{
				{Role: "user", Content: []ContentBlock{{Type: "text", Text: "Compare Tokyo and Osaka."}}},
				{Role: "assistant", Content: normalized.Content},
				{Role: "tool", Content: results},
			}}
			payload, err := TranslateRequest(proto, request)
			if err != nil {
				t.Fatalf("translate: %v", err)
			}
			serialized, err := json.Marshal(payload)
			if err != nil {
				t.Fatalf("marshal payload: %v", err)
			}
			body := string(serialized)
			for _, call := range exp.Responses[name].ToolCalls {
				// Ollama keys tool results by function name, not by call id, so the
				// synthesised id is intentionally not echoed onto the wire.
				if proto == native.ProtocolOllama {
					if !strings.Contains(body, call.Name) {
						t.Fatalf("payload lost tool call %s: %s", call.Name, body)
					}
					continue
				}
				if !strings.Contains(body, call.ID) {
					t.Fatalf("payload lost tool call id %s: %s", call.ID, body)
				}
			}
		})
	}
}

func TestStructuredOutputAndMultimodalFailWithTypedErrors(t *testing.T) {
	var doc struct {
		Request Request `json:"request"`
	}
	if err := json.Unmarshal(loadAgentJSON(t, filepath.Join("requests", "multimodal-structured.json")), &doc); err != nil {
		t.Fatalf("decode request fixture: %v", err)
	}
	wantFeature := map[string]string{
		"anthropic_messages": "json_schema",
		"google_gemini":      "image_url",
		"google_vertex_ai":   "image_url",
		"aws_bedrock":        "image_url",
		"ollama":             "json_schema_strict",
		"cohere":             "image_base64",
	}
	okProtocols := map[string]bool{
		"openai_chat_completions": true,
		"openai_responses":        true,
		"azure_openai":            true,
	}
	for name, feature := range wantFeature {
		proto := protocolOf(t, name)
		_, err := TranslateRequest(proto, doc.Request)
		var unsupported *UnsupportedFeatureError
		if !errors.As(err, &unsupported) {
			t.Fatalf("%s: expected a typed unsupported error, got %v", name, err)
		}
		if unsupported.Feature != feature {
			t.Fatalf("%s: feature = %q, want %q", name, unsupported.Feature, feature)
		}
	}
	for name := range okProtocols {
		proto := protocolOf(t, name)
		if _, err := TranslateRequest(proto, doc.Request); err != nil {
			t.Fatalf("%s: expected a payload, got %v", name, err)
		}
	}
}

func TestForcedToolChoiceIsTranslatedOrRejectedExplicitly(t *testing.T) {
	var doc struct {
		Request Request `json:"request"`
	}
	if err := json.Unmarshal(loadAgentJSON(t, filepath.Join("requests", "forced-tool.json")), &doc); err != nil {
		t.Fatalf("decode request fixture: %v", err)
	}
	for name, feature := range map[string]string{"ollama": "tool_choice", "cohere": "tool_choice_tool"} {
		proto := protocolOf(t, name)
		_, err := TranslateRequest(proto, doc.Request)
		var unsupported *UnsupportedFeatureError
		if !errors.As(err, &unsupported) {
			t.Fatalf("%s: expected a typed unsupported error, got %v", name, err)
		}
		if unsupported.Feature != feature {
			t.Fatalf("%s: feature = %q, want %q", name, unsupported.Feature, feature)
		}
	}
	for _, name := range []string{"openai_chat_completions", "openai_responses", "anthropic_messages", "google_gemini", "google_vertex_ai", "aws_bedrock", "azure_openai"} {
		proto := protocolOf(t, name)
		payload, err := TranslateRequest(proto, doc.Request)
		if err != nil {
			t.Fatalf("%s: expected a payload, got %v", name, err)
		}
		serialized, err := json.Marshal(payload)
		if err != nil {
			t.Fatalf("%s: marshal: %v", name, err)
		}
		if !strings.Contains(string(serialized), "get_weather") {
			t.Fatalf("%s: payload dropped the forced tool: %s", name, serialized)
		}
	}
}
