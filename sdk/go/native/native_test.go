package native_test

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/LingXiFox/LingXiModelSDK/sdk/go/native"
)

func readNativeFixture(t *testing.T, name string) []byte {
	t.Helper()
	b, err := os.ReadFile(filepath.Join("..", "..", "..", "conformance", "native", name))
	if err != nil {
		t.Fatal(err)
	}
	return b
}

func TestFourNativeProtocolsAndRegistry(t *testing.T) {
	var lastPath string
	var lastAuth string
	var mode string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		lastPath = r.URL.RequestURI()
		lastAuth = r.Header.Get("Authorization") + r.Header.Get("x-api-key") + r.Header.Get("x-goog-api-key")
		switch mode {
		case "chat_json":
			_, _ = w.Write(readNativeFixture(t, "openai_chat_response.json"))
		case "chat_sse":
			w.Header().Set("Content-Type", "text/event-stream")
			_, _ = w.Write(readNativeFixture(t, "openai_chat_stream.sse"))
		case "resp_json":
			_, _ = w.Write(readNativeFixture(t, "openai_responses_response.json"))
		case "resp_sse":
			w.Header().Set("Content-Type", "text/event-stream")
			_, _ = w.Write(readNativeFixture(t, "openai_responses_stream.sse"))
		case "ant_json":
			_, _ = w.Write(readNativeFixture(t, "anthropic_messages_response.json"))
		case "ant_sse":
			w.Header().Set("Content-Type", "text/event-stream")
			_, _ = w.Write(readNativeFixture(t, "anthropic_messages_stream.sse"))
		case "gem_json":
			_, _ = w.Write(readNativeFixture(t, "google_gemini_response.json"))
		case "gem_sse":
			w.Header().Set("Content-Type", "text/event-stream")
			_, _ = w.Write(readNativeFixture(t, "google_gemini_stream.sse"))
		case "429":
			w.WriteHeader(http.StatusTooManyRequests)
			_, _ = w.Write(readNativeFixture(t, "error_429.json"))
		}
	}))
	defer srv.Close()

	client := native.NewClient()
	ctx := context.Background()

	mode = "chat_json"
	resp, err := client.OpenAIChatCompletions(ctx, native.RequestConfig{BaseURL: srv.URL + "/v1", APIKey: "sk-1"}, map[string]any{"model": "gpt-5-mini"})
	if err != nil || lastPath != "/v1/chat/completions" || lastAuth != "Bearer sk-1" || resp.Body["id"] != "chatcmpl-mock-1" {
		t.Fatalf("chat_json failed: err=%v path=%s", err, lastPath)
	}

	mode = "chat_sse"
	evs, err := client.StreamOpenAIChatCompletions(ctx, native.RequestConfig{BaseURL: srv.URL + "/v1", APIKey: "sk-1"}, map[string]any{"model": "gpt-5-mini"})
	if err != nil || len(evs) != 3 || evs[2].Data != "[DONE]" {
		t.Fatalf("chat_sse failed: %v %+v", err, evs)
	}

	mode = "resp_json"
	resp, err = client.OpenAIResponses(ctx, native.RequestConfig{BaseURL: srv.URL + "/v1", APIKey: "sk-2"}, map[string]any{"model": "gpt-5"})
	if err != nil || lastPath != "/v1/responses" || resp.Body["id"] != "resp-mock-1" {
		t.Fatalf("resp_json failed: %v", err)
	}

	mode = "resp_sse"
	evs, err = client.StreamOpenAIResponses(ctx, native.RequestConfig{BaseURL: srv.URL + "/v1", APIKey: "sk-2"}, map[string]any{"model": "gpt-5"})
	if err != nil || len(evs) != 4 || evs[1].Event != "response.output_text.delta" {
		t.Fatalf("resp_sse failed: %v", err)
	}

	mode = "ant_json"
	resp, err = client.AnthropicMessages(ctx, native.RequestConfig{BaseURL: srv.URL, APIKey: "sk-ant"}, map[string]any{"model": "claude-sonnet-4-5"})
	if err != nil || lastPath != "/v1/messages" || lastAuth != "sk-ant" || resp.Body["id"] != "msg_mock_1" {
		t.Fatalf("ant_json failed: %v %s", err, lastPath)
	}

	mode = "ant_sse"
	evs, err = client.StreamAnthropicMessages(ctx, native.RequestConfig{BaseURL: srv.URL, APIKey: "sk-ant"}, map[string]any{"model": "claude-sonnet-4-5"})
	if err != nil || len(evs) != 4 || !strings.Contains(evs[1].Data, "thinking_delta") {
		t.Fatalf("ant_sse failed: %v", err)
	}

	mode = "gem_json"
	resp, err = client.GoogleGeminiGenerate(ctx, native.RequestConfig{BaseURL: srv.URL, APIKey: "gem-1"}, "gemini-2.5-pro", map[string]any{"contents": []any{}})
	if err != nil || lastPath != "/v1beta/models/gemini-2.5-pro:generateContent" || lastAuth != "gem-1" {
		t.Fatalf("gem_json failed: %v %s", err, lastPath)
	}

	mode = "gem_sse"
	evs, err = client.StreamGoogleGeminiGenerate(ctx, native.RequestConfig{BaseURL: srv.URL, APIKey: "gem-1"}, "gemini-2.5-pro", map[string]any{"contents": []any{}})
	if err != nil || len(evs) != 2 || lastPath != "/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse" {
		t.Fatalf("gem_sse failed: %v %s", err, lastPath)
	}

	// Declarative Registry dispatch + 429 error body preservation
	regBytes, err := os.ReadFile(filepath.Join("..", "..", "..", "registry", "registry.json"))
	if err != nil {
		t.Fatal(err)
	}
	reg, err := native.ParseRegistry(regBytes)
	if err != nil || reg.Len() < 300 {
		t.Fatalf("registry load failed: %v len=%d", err, reg.Len())
	}
	mode = "429"
	_, err = client.SendRegisteredJSON(ctx, reg, "deepseek", native.ProtocolOpenAIChatCompletions, "deepseek-reasoner", "sk-ds", srv.URL, map[string]any{"model": "deepseek-reasoner"})
	var httpErr *native.HTTPStatusError
	if !errors.As(err, &httpErr) || httpErr.Status != 429 || !strings.Contains(string(httpErr.Body), "rate_limit_exceeded") {
		t.Fatalf("expected 429 error body preserved, got %v", err)
	}
}
