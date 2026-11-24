package unified

import (
	"context"

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
	default:
		return nil, &UnsupportedFeatureError{Protocol: string(proto), Feature: "protocol"}
	}
	if err != nil {
		return nil, err
	}
	return NormalizeResponse(proto, resp.Body)
}

func (c *Client) Stream(ctx context.Context, proto native.Protocol, baseURL, apiKey string, req Request) ([]StreamEvent, error) {
	payload, err := TranslateRequest(proto, req)
	if err != nil {
		return nil, err
	}
	cfg := native.RequestConfig{BaseURL: baseURL, APIKey: apiKey}
	var events []native.SSEEvent
	switch proto {
	case native.ProtocolOpenAIChatCompletions:
		events, err = c.Native.StreamOpenAIChatCompletions(ctx, cfg, payload)
	case native.ProtocolOpenAIResponses:
		events, err = c.Native.StreamOpenAIResponses(ctx, cfg, payload)
	case native.ProtocolAnthropicMessages:
		events, err = c.Native.StreamAnthropicMessages(ctx, cfg, payload)
	case native.ProtocolGoogleGemini:
		events, err = c.Native.StreamGoogleGeminiGenerate(ctx, cfg, req.Model, payload)
	default:
		return nil, &UnsupportedFeatureError{Protocol: string(proto), Feature: "protocol"}
	}
	if err != nil {
		return nil, err
	}
	return NormalizeStreamEvents(proto, events)
}
