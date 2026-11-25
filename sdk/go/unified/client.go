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
	// One stateful normalizer per stream: tool arguments arrive in fragments,
	// thinking signatures may trail their text, and message_start/message_end
	// must be emitted exactly once.
	normalizer := NewStreamNormalizer(proto)
	deliver := func(events []StreamEvent) error {
		for _, item := range events {
			if err := fn(item); err != nil {
				return err
			}
		}
		return nil
	}
	emit := func(raw native.SSEEvent) error {
		parsed, err := normalizer.Push(raw)
		if err != nil {
			return err
		}
		return deliver(parsed)
	}
	finish := func(err error) error {
		if err != nil {
			return err
		}
		return deliver(normalizer.Finish())
	}
	switch proto {
	case native.ProtocolOpenAIChatCompletions:
		return finish(c.Native.StreamOpenAIChatCompletionsEach(ctx, cfg, payload, emit))
	case native.ProtocolOpenAIResponses:
		return finish(c.Native.StreamOpenAIResponsesEach(ctx, cfg, payload, emit))
	case native.ProtocolAnthropicMessages:
		return finish(c.Native.StreamAnthropicMessagesEach(ctx, cfg, payload, emit))
	case native.ProtocolGoogleGemini:
		return finish(c.Native.StreamGoogleGeminiGenerateEach(ctx, cfg, req.Model, payload, emit))
	case native.ProtocolAWSBedrock:
		return finish(c.Native.StreamAWSBedrockConverseEach(ctx, cfg, req.Model, payload, emit))
	case native.ProtocolGoogleVertexAI:
		return finish(c.Native.StreamGoogleVertexGenerateEach(ctx, cfg, req.Model, payload, emit))
	case native.ProtocolAzureOpenAI:
		return finish(c.Native.StreamAzureOpenAIChatCompletionsEach(ctx, cfg, req.Model, payload, emit))
	case native.ProtocolOllama:
		return finish(c.Native.StreamOllamaChatEach(ctx, cfg, payload, emit))
	case native.ProtocolCohere:
		return finish(c.Native.StreamCohereChatEach(ctx, cfg, payload, emit))
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
		model := "models/" + strings.TrimPrefix(req.Model, "models/")
		url = fmt.Sprintf("%s/v1beta/%s:embedContent", base, model)
		if len(req.Input) == 1 {
			payload = map[string]any{"content": map[string]any{"parts": []map[string]any{{"text": req.Input[0]}}}}
		} else {
			url = fmt.Sprintf("%s/v1beta/%s:batchEmbedContents", base, model)
			requests := make([]map[string]any, 0, len(req.Input))
			for _, input := range req.Input {
				requests = append(requests, map[string]any{"model": model, "content": map[string]any{"parts": []map[string]any{{"text": input}}}})
			}
			payload = map[string]any{"requests": requests}
		}
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
	out, err := normalizeEmbeddingResponse(proto, resp.Body)
	if err == nil && proto == native.ProtocolGoogleGemini && len(out.Embeddings) != len(req.Input) {
		return nil, fmt.Errorf("embedding count does not match request")
	}
	return out, err
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
	case native.ProtocolGoogleGemini:
		rows, ok := raw["embeddings"].([]any)
		if !ok {
			rows = []any{raw["embedding"]}
		}
		for _, row := range rows {
			emb, _ := row.(map[string]any)
			values, _ := emb["values"].([]any)
			vector := floatSlice(values)
			if len(vector) == 0 || len(vector) != len(values) {
				return nil, fmt.Errorf("invalid embedding.values")
			}
			embeddings = append(embeddings, vector)
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
