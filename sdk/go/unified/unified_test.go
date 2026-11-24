package unified_test

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"

	"github.com/LingXiFox/LingXiModelSDK/sdk/go/native"
	"github.com/LingXiFox/LingXiModelSDK/sdk/go/unified"
)

func readNativeFixture(t *testing.T, name string) []byte {
	t.Helper()
	b, err := os.ReadFile(filepath.Join("..", "..", "..", "conformance", "native", name))
	if err != nil {
		t.Fatal(err)
	}
	return b
}

func TestUnifiedLMPConformanceAndUnsupportedGuards(t *testing.T) {
	expBytes, err := os.ReadFile(filepath.Join("..", "..", "..", "conformance", "lmp", "expectations.json"))
	if err != nil {
		t.Fatal(err)
	}
	var exp struct {
		NonStreaming map[string]struct {
			Text            string `json:"text"`
			Thinking        string `json:"thinking"`
			FinishReason    string `json:"finishReason"`
			InputTokens     int64  `json:"inputTokens"`
			OutputTokens    int64  `json:"outputTokens"`
			CacheReadTokens *int64 `json:"cacheReadTokens"`
		} `json:"nonStreaming"`
		StreamingText map[string]string `json:"streamingText"`
	}
	if err := json.Unmarshal(expBytes, &exp); err != nil {
		t.Fatal(err)
	}

	var currentBody []byte
	var currentCT string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", currentCT)
		_, _ = w.Write(currentBody)
	}))
	defer srv.Close()

	client := unified.NewClient()
	ctx := context.Background()

	cases := []struct {
		proto    native.Protocol
		key      string
		respFile string
		sseFile  string
		model    string
	}{
		{native.ProtocolOpenAIChatCompletions, "openai_chat_completions", "openai_chat_response.json", "openai_chat_stream.sse", "gpt-5-mini"},
		{native.ProtocolOpenAIResponses, "openai_responses", "openai_responses_response.json", "openai_responses_stream.sse", "gpt-5"},
		{native.ProtocolAnthropicMessages, "anthropic_messages", "anthropic_messages_response.json", "anthropic_messages_stream.sse", "claude-sonnet-4-5"},
		{native.ProtocolGoogleGemini, "google_gemini", "google_gemini_response.json", "google_gemini_stream.sse", "gemini-2.5-pro"},
	}

	for _, tc := range cases {
		req := unified.Request{
			Model:    tc.model,
			Messages: []unified.Message{{Role: "user", Content: []unified.ContentBlock{{Type: "text", Text: "Hello"}}}},
		}
		currentCT = "application/json"
		currentBody = readNativeFixture(t, tc.respFile)
		resp, err := client.Complete(ctx, tc.proto, srv.URL, "k", req)
		if err != nil {
			t.Fatalf("%s complete failed: %v", tc.key, err)
		}
		want := exp.NonStreaming[tc.key]
		if resp.Text() != want.Text || resp.ThinkingText() != want.Thinking || resp.FinishReason != want.FinishReason || resp.Usage.InputTokens != want.InputTokens {
			t.Fatalf("%s mismatch: got text=%q thinking=%q fr=%q in=%d", tc.key, resp.Text(), resp.ThinkingText(), resp.FinishReason, resp.Usage.InputTokens)
		}
		if want.CacheReadTokens != nil && (resp.Usage.CacheReadTokens == nil || *resp.Usage.CacheReadTokens != *want.CacheReadTokens) {
			t.Fatalf("%s cacheReadTokens mismatch", tc.key)
		}

		currentCT = "text/event-stream"
		currentBody = readNativeFixture(t, tc.sseFile)
		evs, err := client.Stream(ctx, tc.proto, srv.URL, "k", req)
		if err != nil {
			t.Fatalf("%s stream failed: %v", tc.key, err)
		}
		var streamed string
		for _, ev := range evs {
			if ev.Type == "text_delta" {
				streamed += ev.Delta
			}
		}
		if streamed != exp.StreamingText[tc.key] {
			t.Fatalf("%s streamed text got %q want %q", tc.key, streamed, exp.StreamingText[tc.key])
		}
	}

	_, err = client.Complete(ctx, native.ProtocolAnthropicMessages, srv.URL, "k", unified.Request{
		Model:          "claude-sonnet-4-5",
		Messages:       []unified.Message{{Role: "user", Content: []unified.ContentBlock{{Type: "text", Text: "hi"}}}},
		ResponseFormat: &unified.ResponseFormat{Type: "json_schema"},
	})
	var unsup *unified.UnsupportedFeatureError
	if !errors.As(err, &unsup) || unsup.Feature != "json_schema" {
		t.Fatalf("expected UnsupportedFeatureError for anthropic json_schema, got %v", err)
	}
}
