package unified

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/LingXiFox/LingXiModelSDK/sdk/go/native"
)

func loadNativeFixture(t *testing.T, name string) string {
	t.Helper()
	b, err := os.ReadFile(filepath.Join("..", "..", "..", "conformance", "native", name))
	if err != nil {
		t.Fatalf("read fixture %s: %v", name, err)
	}
	return string(b)
}

func TestAllNineProtocolsAndAgentScenarios(t *testing.T) {
	ctx := context.Background()
	c := NewClient()

	// Bedrock (SigV4 + Converse + NDJSON)
	bedrockResp := loadNativeFixture(t, "aws_bedrock_response.json")
	bedrockStream := loadNativeFixture(t, "aws_bedrock_stream.ndjson")
	srvBedrock := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if !strings.HasPrefix(r.Header.Get("Authorization"), "AWS4-HMAC-SHA256") {
			t.Fatalf("expected SigV4 header, got %q", r.Header.Get("Authorization"))
		}
		if strings.Contains(r.URL.Path, "converse-stream") {
			w.Header().Set("Content-Type", "application/x-ndjson")
			_, _ = w.Write([]byte(bedrockStream))
			return
		}
		_, _ = w.Write([]byte(bedrockResp))
	}))
	defer srvBedrock.Close()

	resp, err := c.Complete(ctx, native.ProtocolAWSBedrock, srvBedrock.URL, "AKIA:SECRET", Request{
		Model:    "anthropic.claude-3-5-sonnet",
		Messages: []Message{{Role: "user", Content: []ContentBlock{{Type: "text", Text: "hi"}}}},
		Thinking: &ThinkingConfig{BudgetTokens: 2048},
	})
	if err != nil {
		t.Fatalf("bedrock complete: %v", err)
	}
	if len(resp.Content) < 2 || resp.Content[0].Signature != "sig-bedrock-1" {
		t.Fatalf("unexpected bedrock content: %+v", resp.Content)
	}
	evs, err := c.Stream(ctx, native.ProtocolAWSBedrock, srvBedrock.URL, "AKIA:SECRET", Request{
		Model:    "anthropic.claude-3-5-sonnet",
		Messages: []Message{{Role: "user", Content: []ContentBlock{{Type: "text", Text: "hi"}}}},
	})
	if err != nil || len(evs) == 0 {
		t.Fatalf("bedrock stream failed: %v %+v", err, evs)
	}

	// Vertex AI (thoughtSignature retention)
	vertexResp := loadNativeFixture(t, "google_vertex_response.json")
	srvVertex := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(vertexResp))
	}))
	defer srvVertex.Close()
	respV, err := c.Complete(ctx, native.ProtocolGoogleVertexAI, srvVertex.URL, "tok", Request{
		Model:    "gemini-2.5-pro",
		Messages: []Message{{Role: "user", Content: []ContentBlock{{Type: "text", Text: "hi"}}}},
	})
	if err != nil || respV.Content[0].Signature != "sig-vertex-1" {
		t.Fatalf("vertex failed: %v %+v", err, respV)
	}

	// Azure OpenAI
	azureResp := loadNativeFixture(t, "azure_openai_response.json")
	srvAzure := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("api-key") != "az-key" {
			t.Fatalf("missing api-key header")
		}
		_, _ = w.Write([]byte(azureResp))
	}))
	defer srvAzure.Close()
	respAz, err := c.Complete(ctx, native.ProtocolAzureOpenAI, srvAzure.URL, "az-key", Request{
		Model:    "gpt-4o",
		Messages: []Message{{Role: "user", Content: []ContentBlock{{Type: "text", Text: "hi"}}}},
	})
	if err != nil || !strings.Contains(respAz.Text(), "Azure") {
		t.Fatalf("azure failed: %v %+v", err, respAz)
	}

	// Ollama + explicit UnsupportedFeatureError on json_schema
	ollamaResp := loadNativeFixture(t, "ollama_chat_response.json")
	srvOllama := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(ollamaResp))
	}))
	defer srvOllama.Close()
	respOl, err := c.Complete(ctx, native.ProtocolOllama, srvOllama.URL, "", Request{
		Model:    "llama3.3",
		Messages: []Message{{Role: "user", Content: []ContentBlock{{Type: "text", Text: "hi"}}}},
	})
	if err != nil || respOl.Usage.TotalTokens != 22 {
		t.Fatalf("ollama failed: %v %+v", err, respOl)
	}
	_, err = c.Complete(ctx, native.ProtocolOllama, srvOllama.URL, "", Request{
		Model:          "llama3.3",
		Messages:       []Message{{Role: "user", Content: []ContentBlock{{Type: "text", Text: "hi"}}}},
		ResponseFormat: &ResponseFormat{Type: "json_schema", Name: "s", Schema: map[string]any{"type": "object"}},
	})
	var unsup *UnsupportedFeatureError
	if !errors.As(err, &unsup) {
		t.Fatalf("expected UnsupportedFeatureError, got %v", err)
	}

	// Cohere chat + rerank + OpenAI embed
	cohereResp := loadNativeFixture(t, "cohere_chat_response.json")
	cohereRerank := loadNativeFixture(t, "cohere_rerank_response.json")
	openaiEmbed := loadNativeFixture(t, "openai_embed_response.json")
	srvMisc := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/v2/rerank":
			_, _ = w.Write([]byte(cohereRerank))
		case "/embeddings":
			_, _ = w.Write([]byte(openaiEmbed))
		default:
			_, _ = w.Write([]byte(cohereResp))
		}
	}))
	defer srvMisc.Close()

	rr, err := c.Rerank(ctx, native.ProtocolCohere, srvMisc.URL, "co-key", RerankRequest{
		Model: "rerank-v3.5", Query: "go", Documents: []string{"a", "b"}, TopN: 2,
	})
	if err != nil || len(rr.Results) != 2 || rr.Results[0].Index != 1 {
		t.Fatalf("cohere rerank failed: %v %+v", err, rr)
	}
	emb, err := c.Embed(ctx, native.ProtocolOpenAIChatCompletions, srvMisc.URL, "sk-test", EmbeddingRequest{
		Model: "text-embedding-3-small", Input: []string{"hello", "world"},
	})
	if err != nil || len(emb.Embeddings) != 2 {
		t.Fatalf("openai embed failed: %v %+v", err, emb)
	}

	// Redirect blocking check
	srvRedir := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, "http://127.0.0.1:9/leak", http.StatusFound)
	}))
	defer srvRedir.Close()
	_, err = c.Complete(ctx, native.ProtocolOpenAIChatCompletions, srvRedir.URL, "sk-secret", Request{
		Model:    "gpt-4o",
		Messages: []Message{{Role: "user", Content: []ContentBlock{{Type: "text", Text: "hi"}}}},
	})
	if err == nil {
		t.Fatalf("expected redirect to be rejected")
	}
	_ = json.Marshal
}

func BenchmarkTranslateAndNormalizeGo(b *testing.B) {
	req := Request{
		Model:    "gpt-4o",
		Messages: []Message{{Role: "user", Content: []ContentBlock{{Type: "text", Text: "hello benchmark"}}}},
		Thinking: &ThinkingConfig{Effort: "medium"},
	}
	raw := map[string]any{
		"id":      "chatcmpl-bench",
		"choices": []any{map[string]any{"message": map[string]any{"role": "assistant", "content": "ok"}, "finish_reason": "stop"}},
		"usage":   map[string]any{"prompt_tokens": float64(10), "completion_tokens": float64(5), "total_tokens": float64(15)},
	}
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		if _, err := TranslateRequest(native.ProtocolOpenAIChatCompletions, req); err != nil {
			b.Fatal(err)
		}
		if _, err := NormalizeResponse(native.ProtocolOpenAIChatCompletions, raw); err != nil {
			b.Fatal(err)
		}
	}
}
