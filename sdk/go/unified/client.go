package unified

import (
	"context"
	"fmt"
	"strings"

	"github.com/LingXiFox/LingXiModelSDK/sdk/go/native"
)

// Client provides opt-in normalized LMP operations over a pooled native.Client.
type Client struct {
	Native *native.Client
}

// NewClient constructs a Unified Client sharing a single HTTP connection pool.
func NewClient() *Client {
	return &Client{Native: native.NewClient()}
}

func (c *Client) Complete(ctx context.Context, proto native.Protocol, baseURL, apiKey string, req Request) (*Response, error) {
	payload, err := TranslateRequest(proto, req)
	if err != nil {
		return nil, err
	}
	cfg := native.RequestConfig{BaseURL: baseURL, APIKey: apiKey}
	var resp *native.JSONResponse
	switch proto {
	case native.ProtocolOpenAIChatCompletions:
		resp, err = c.Native.OpenAIChatCompletions(ctx, cfg, payload)
	case native.ProtocolOpenAIResponses:
		resp, err = c.Native.OpenAIResponses(ctx, cfg, payload)
	case native.ProtocolAnthropicMessages:
		resp, err = c.Native.AnthropicMessages(ctx, cfg, payload)
	case native.ProtocolGoogleGemini:
		resp, err = c.Native.GoogleGeminiGenerate(ctx, cfg, req.Model, payload)
	case native.ProtocolAWSBedrock:
		resp, err = c.Native.AWSBedrockConverse(ctx, cfg, req.Model, payload)
	case native.ProtocolGoogleVertexAI:
		resp, err = c.Native.GoogleVertexGenerate(ctx, cfg, req.Model, payload)
	case native.ProtocolAzureOpenAI:
		resp, err = c.Native.AzureOpenAIChatCompletions(ctx, cfg, req.Model, payload)
	case native.ProtocolOllama:
		resp, err = c.Native.OllamaChat(ctx, cfg, payload)
	case native.ProtocolCohere:
		resp, err = c.Native.CohereChat(ctx, cfg, payload)
	default:
		return nil, &UnsupportedFeatureError{Protocol: string(proto), Feature: "protocol"}
	}
	if err != nil {
		return nil, err
	}
	return NormalizeResponse(proto, resp.Body)
}

func (c *Client) Stream(ctx context.Context, proto native.Protocol, baseURL, apiKey string, req Request) ([]StreamEvent, error) {
	var out []StreamEvent
	err := c.StreamEach(ctx, proto, baseURL, apiKey, req, func(ev StreamEvent) error {
		out = append(out, ev)
		return nil
	})
	return out, err
}

// StreamEach streams normalized LMP events in real time without buffering the full response.
func (c *Client) StreamEach(ctx context.Context, proto native.Protocol, baseURL, apiKey string, req Request, fn func(StreamEvent) error) error {
	payload, err := TranslateRequest(proto, req)
	if err != nil {
		return err
	}
	cfg := native.RequestConfig{BaseURL: baseURL, APIKey: apiKey}
	emit := func(raw native.SSEEvent) error {
		parsed, err := NormalizeStreamEvent(proto, raw)
		if err != nil {
			return err
		}
		for _, item := range parsed {
			if err := fn(item); err != nil {
				return err
			}
		}
		return nil
	}
	switch proto {
	case native.ProtocolOpenAIChatCompletions:
		return c.Native.StreamOpenAIChatCompletionsEach(ctx, cfg, payload, emit)
	case native.ProtocolOpenAIResponses:
		return c.Native.StreamOpenAIResponsesEach(ctx, cfg, payload, emit)
	case native.ProtocolAnthropicMessages:
		return c.Native.StreamAnthropicMessagesEach(ctx, cfg, payload, emit)
	case native.ProtocolGoogleGemini:
		return c.Native.StreamGoogleGeminiGenerateEach(ctx, cfg, req.Model, payload, emit)
	case native.ProtocolAWSBedrock:
		return c.Native.StreamAWSBedrockConverseEach(ctx, cfg, req.Model, payload, emit)
	case native.ProtocolGoogleVertexAI:
		return c.Native.StreamGoogleVertexGenerateEach(ctx, cfg, req.Model, payload, emit)
	case native.ProtocolAzureOpenAI:
		return c.Native.StreamAzureOpenAIChatCompletionsEach(ctx, cfg, req.Model, payload, emit)
	case native.ProtocolOllama:
		return c.Native.StreamOllamaChatEach(ctx, cfg, payload, emit)
	case native.ProtocolCohere:
		return c.Native.StreamCohereChatEach(ctx, cfg, payload, emit)
	default:
		return &UnsupportedFeatureError{Protocol: string(proto), Feature: "protocol"}
	}
}

func (c *Client) Embed(ctx context.Context, proto native.Protocol, baseURL, apiKey string, req EmbeddingRequest) (*EmbeddingResponse, error) {
	if len(req.Input) == 0 {
		return nil, fmt.Errorf("embedding input is empty")
	}
	base := strings.TrimRight(baseURL, "/")
	headers := map[string]string{}
	var url string
	var payload map[string]any

	switch proto {
	case native.ProtocolOpenAIChatCompletions:
		headers["Authorization"] = "Bearer " + apiKey
		url = base + "/embeddings"
		payload = map[string]any{"model": req.Model, "input": req.Input}
	case native.ProtocolAzureOpenAI:
		headers["api-key"] = apiKey
		url = fmt.Sprintf("%s/openai/deployments/%s/embeddings?api-version=2024-10-21", base, req.Model)
		payload = map[string]any{"input": req.Input}
	case native.ProtocolGoogleGemini:
		headers["x-goog-api-key"] = apiKey
		url = fmt.Sprintf("%s/v1beta/models/%s:embedContent", base, req.Model)
		payload = map[string]any{"content": map[string]any{"parts": []map[string]any{{"text": req.Input[0]}}}}
	case native.ProtocolGoogleVertexAI:
		headers["Authorization"] = "Bearer " + apiKey
		url = fmt.Sprintf("%s/v1/publishers/google/models/%s:embedContent", base, req.Model)
		payload = map[string]any{"content": map[string]any{"parts": []map[string]any{{"text": req.Input[0]}}}}
	case native.ProtocolOllama:
		url = base + "/api/embed"
		payload = map[string]any{"model": req.Model, "input": req.Input}
	case native.ProtocolCohere:
		headers["Authorization"] = "Bearer " + apiKey
		url = base + "/v2/embed"
		payload = map[string]any{"model": req.Model, "texts": req.Input, "input_type": "search_document"}
	default:
		return nil, &UnsupportedFeatureError{Protocol: string(proto), Feature: "embed"}
	}

	resp, err := c.Native.PostJSONEndpoint(ctx, url, headers, payload, false)
	if err != nil {
		return nil, err
	}
	return normalizeEmbeddingResponse(proto, resp.Body)
}

func (c *Client) Rerank(ctx context.Context, proto native.Protocol, baseURL, apiKey string, req RerankRequest) (*RerankResponse, error) {
	if proto != native.ProtocolCohere {
		return nil, &UnsupportedFeatureError{Protocol: string(proto), Feature: "rerank"}
	}
	headers := map[string]string{"Authorization": "Bearer " + apiKey}
	payload := map[string]any{"model": req.Model, "query": req.Query, "documents": req.Documents}
	if req.TopN > 0 {
		payload["top_n"] = req.TopN
	}
	resp, err := c.Native.PostJSONEndpoint(ctx, strings.TrimRight(baseURL, "/")+"/v2/rerank", headers, payload, false)
	if err != nil {
		return nil, err
	}
	rawResults, _ := resp.Body["results"].([]any)
	results := make([]RerankResult, 0, len(rawResults))
	for _, r := range rawResults {
		m, _ := r.(map[string]any)
		idx, _ := m["index"].(float64)
		score, _ := m["relevance_score"].(float64)
		results = append(results, RerankResult{Index: int(idx), RelevanceScore: score})
	}
	return &RerankResponse{Results: results, Raw: resp.Body}, nil
}

func normalizeEmbeddingResponse(proto native.Protocol, raw map[string]any) (*EmbeddingResponse, error) {
	var embeddings [][]float64
	switch proto {
	case native.ProtocolOpenAIChatCompletions, native.ProtocolAzureOpenAI:
		data, _ := raw["data"].([]any)
		for _, item := range data {
			m, _ := item.(map[string]any)
			embeddings = append(embeddings, floatSlice(m["embedding"]))
		}
	case native.ProtocolGoogleGemini, native.ProtocolGoogleVertexAI:
		if emb, ok := raw["embedding"].(map[string]any); ok {
			embeddings = append(embeddings, floatSlice(emb["values"]))
		}
	case native.ProtocolOllama:
		rows, _ := raw["embeddings"].([]any)
		for _, row := range rows {
			embeddings = append(embeddings, floatSlice(row))
		}
	case native.ProtocolCohere:
		if emb, ok := raw["embeddings"].(map[string]any); ok {
			rows, _ := emb["float"].([]any)
			for _, row := range rows {
				embeddings = append(embeddings, floatSlice(row))
			}
		}
	}
	return &EmbeddingResponse{Embeddings: embeddings, Raw: raw}, nil
}

func floatSlice(raw any) []float64 {
	arr, _ := raw.([]any)
	out := make([]float64, 0, len(arr))
	for _, v := range arr {
		if f, ok := v.(float64); ok {
			out = append(out, f)
		}
	}
	return out
}
