package unified

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
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

	// Bedrock (SigV4 + Converse + Amazon Event Stream)
	bedrockResp := loadNativeFixture(t, "aws_bedrock_response.json")
	bedrockStream := loadNativeFixture(t, "aws_bedrock_stream.eventstream")
	srvBedrock := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if !strings.HasPrefix(r.Header.Get("Authorization"), "AWS4-HMAC-SHA256") {
			t.Fatalf("expected SigV4 header, got %q", r.Header.Get("Authorization"))
		}
		if strings.Contains(r.URL.Path, "converse-stream") {
			w.Header().Set("Content-Type", "application/vnd.amazon.eventstream")
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

func TestGeminiReplaysParallelToolCallsAndOriginalSignedParts(t *testing.T) {
	data, err := os.ReadFile(filepath.Join("..", "..", "..", "conformance", "agent", "gemini-tool-loop.json"))
	if err != nil {
		t.Fatal(err)
	}
	var fixture struct {
		Request                      Request
		FirstResponse, FinalResponse json.RawMessage
		ExpectedToolResponses        []any
	}
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatal(err)
	}
	var firstWire struct {
		Candidates []struct{ Content map[string]any }
	}
	if err := json.Unmarshal(fixture.FirstResponse, &firstWire); err != nil {
		t.Fatal(err)
	}
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var payload struct {
			Contents         []map[string]any
			GenerationConfig struct{ ThinkingConfig map[string]any }
			CachedContent    string
		}
		if err := json.NewDecoder(r.Body).Decode(&payload); err != nil {
			t.Error(err)
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		if r.Header.Get("x-goog-api-key") != "fixture" || payload.CachedContent != "cachedContents/mock-1" || !reflect.DeepEqual(payload.GenerationConfig.ThinkingConfig, map[string]any{"thinkingLevel": "LOW", "includeThoughts": true}) {
			t.Errorf("unexpected Gemini configuration: %+v", payload)
		}
		if len(payload.Contents) == 1 {
			if !strings.Contains(fmt.Sprint(payload.Contents[0]["parts"]), "image/png") {
				t.Errorf("missing inline image part: %+v", payload.Contents)
			}
			_, _ = w.Write(fixture.FirstResponse)
			return
		}
		if len(payload.Contents) != 3 || !reflect.DeepEqual(payload.Contents[1], firstWire.Candidates[0].Content) || !reflect.DeepEqual(payload.Contents[2]["parts"], fixture.ExpectedToolResponses) {
			t.Errorf("unexpected tool turn: %+v", payload.Contents)
		}
		_, _ = w.Write(fixture.FinalResponse)
	}))
	defer upstream.Close()
	client := NewClient()
	ctx := context.Background()
	first, err := client.Complete(ctx, native.ProtocolGoogleGemini, upstream.URL, "fixture", fixture.Request)
	if err != nil {
		t.Fatal(err)
	}
	if first.Usage.CacheReadTokens == nil || *first.Usage.CacheReadTokens != 0 || first.Usage.ReasoningTokens == nil || *first.Usage.ReasoningTokens != 5 {
		t.Fatalf("unexpected usage: %+v", first.Usage)
	}
	fixture.Request.Messages = append(fixture.Request.Messages, Message{Role: "assistant", Content: first.Content})
	for _, call := range first.Content {
		if call.Type != "tool_call" {
			continue
		}
		result := "21 C"
		if call.Arguments["city"] == "東京" {
			result = "18 C"
		}
		fixture.Request.Messages = append(fixture.Request.Messages, Message{Role: "tool", Content: []ContentBlock{{Type: "tool_result", ToolCallID: call.ID, Content: result}}})
	}
	last, err := client.Complete(ctx, native.ProtocolGoogleGemini, upstream.URL, "fixture", fixture.Request)
	if err != nil || last.Text() != "Osaka is 3 C warmer." || last.Usage.CacheReadTokens == nil || *last.Usage.CacheReadTokens != 25 {
		t.Fatalf("last=%+v err=%v", last, err)
	}
}

func TestGeminiBatchEmbeddingPreservesInputs(t *testing.T) {
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body struct{ Requests []json.RawMessage }
		err := json.NewDecoder(r.Body).Decode(&body)
		if r.URL.Path != "/v1beta/models/mock:batchEmbedContents" || err != nil ||
			len(body.Requests) != 2 || !strings.Contains(string(body.Requests[1]), "second") {
			t.Errorf("bad batch request: %s %+v %v", r.URL.Path, body, err)
		}
		_, _ = w.Write([]byte(`{"embeddings":[{"values":[0.1]},{"values":[0.2]}]}`))
	}))
	defer upstream.Close()
	client := NewClient()
	request := EmbeddingRequest{Model: "models/mock", Input: []string{"first", "second"}}
	response, err := client.Embed(context.Background(), native.ProtocolGoogleGemini, upstream.URL, "fixture", request)
	if err != nil || !reflect.DeepEqual(response.Embeddings, [][]float64{{0.1}, {0.2}}) {
		t.Fatalf("bad batch response: %+v %v", response, err)
	}
}
