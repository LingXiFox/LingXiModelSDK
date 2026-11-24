package native

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

// RequestConfig configures a direct native upstream call.
type RequestConfig struct {
	BaseURL           string
	APIKey            string
	Headers           map[string]string
	AllowInsecureHTTP bool
}

// JSONResponse holds the raw status, headers, bytes, and decoded JSON map.
type JSONResponse struct {
	Status   int
	Headers  http.Header
	RawBytes []byte
	Body     map[string]any
}

// HTTPStatusError preserves the upstream status code and raw response body.
type HTTPStatusError struct {
	Status  int
	Headers http.Header
	Body    []byte
}

func (e *HTTPStatusError) Error() string {
	return fmt.Sprintf("upstream HTTP %d: %s", e.Status, string(e.Body))
}

// Client communicates directly with upstream providers using native wire protocols.
type Client struct {
	HTTP *http.Client
}

// NewClient constructs a Native Client with a standard HTTP client.
func NewClient() *Client {
	return &Client{HTTP: &http.Client{Timeout: 60 * time.Second}}
}

func (c *Client) OpenAIChatCompletions(ctx context.Context, cfg RequestConfig, payload map[string]any) (*JSONResponse, error) {
	url := strings.TrimRight(cfg.BaseURL, "/") + "/chat/completions"
	headers := cloneHeaders(cfg.Headers)
	if err := applyBearer(headers, cfg.APIKey); err != nil {
		return nil, err
	}
	return c.postJSON(ctx, url, headers, payload, cfg.AllowInsecureHTTP)
}

func (c *Client) StreamOpenAIChatCompletions(ctx context.Context, cfg RequestConfig, payload map[string]any) ([]SSEEvent, error) {
	url := strings.TrimRight(cfg.BaseURL, "/") + "/chat/completions"
	headers := cloneHeaders(cfg.Headers)
	if err := applyBearer(headers, cfg.APIKey); err != nil {
		return nil, err
	}
	return c.postSSE(ctx, url, headers, withStreamTrue(payload), cfg.AllowInsecureHTTP)
}

func (c *Client) OpenAIResponses(ctx context.Context, cfg RequestConfig, payload map[string]any) (*JSONResponse, error) {
	url := strings.TrimRight(cfg.BaseURL, "/") + "/responses"
	headers := cloneHeaders(cfg.Headers)
	if err := applyBearer(headers, cfg.APIKey); err != nil {
		return nil, err
	}
	return c.postJSON(ctx, url, headers, payload, cfg.AllowInsecureHTTP)
}

func (c *Client) StreamOpenAIResponses(ctx context.Context, cfg RequestConfig, payload map[string]any) ([]SSEEvent, error) {
	url := strings.TrimRight(cfg.BaseURL, "/") + "/responses"
	headers := cloneHeaders(cfg.Headers)
	if err := applyBearer(headers, cfg.APIKey); err != nil {
		return nil, err
	}
	return c.postSSE(ctx, url, headers, withStreamTrue(payload), cfg.AllowInsecureHTTP)
}

func (c *Client) AnthropicMessages(ctx context.Context, cfg RequestConfig, payload map[string]any) (*JSONResponse, error) {
	url := joinAnthropicMessages(cfg.BaseURL)
	headers := cloneHeaders(cfg.Headers)
	if err := applyHeaderAuth(headers, "x-api-key", cfg.APIKey); err != nil {
		return nil, err
	}
	if _, ok := headers["anthropic-version"]; !ok {
		headers["anthropic-version"] = "2023-06-01"
	}
	return c.postJSON(ctx, url, headers, payload, cfg.AllowInsecureHTTP)
}

func (c *Client) StreamAnthropicMessages(ctx context.Context, cfg RequestConfig, payload map[string]any) ([]SSEEvent, error) {
	url := joinAnthropicMessages(cfg.BaseURL)
	headers := cloneHeaders(cfg.Headers)
	if err := applyHeaderAuth(headers, "x-api-key", cfg.APIKey); err != nil {
		return nil, err
	}
	if _, ok := headers["anthropic-version"]; !ok {
		headers["anthropic-version"] = "2023-06-01"
	}
	return c.postSSE(ctx, url, headers, withStreamTrue(payload), cfg.AllowInsecureHTTP)
}

func (c *Client) GoogleGeminiGenerate(ctx context.Context, cfg RequestConfig, model string, payload map[string]any) (*JSONResponse, error) {
	url := fmt.Sprintf("%s/v1beta/models/%s:generateContent", strings.TrimRight(cfg.BaseURL, "/"), model)
	headers := cloneHeaders(cfg.Headers)
	if err := applyHeaderAuth(headers, "x-goog-api-key", cfg.APIKey); err != nil {
		return nil, err
	}
	return c.postJSON(ctx, url, headers, payload, cfg.AllowInsecureHTTP)
}

func (c *Client) StreamGoogleGeminiGenerate(ctx context.Context, cfg RequestConfig, model string, payload map[string]any) ([]SSEEvent, error) {
	url := fmt.Sprintf("%s/v1beta/models/%s:streamGenerateContent?alt=sse", strings.TrimRight(cfg.BaseURL, "/"), model)
	headers := cloneHeaders(cfg.Headers)
	if err := applyHeaderAuth(headers, "x-goog-api-key", cfg.APIKey); err != nil {
		return nil, err
	}
	return c.postSSE(ctx, url, headers, payload, cfg.AllowInsecureHTTP)
}

// SendRegisteredJSON dispatches a native call using the declarative Registry.
func (c *Client) SendRegisteredJSON(ctx context.Context, reg *Registry, providerID string, proto Protocol, modelID, apiKey, endpointOverride string, payload map[string]any) (*JSONResponse, error) {
	entry, base, err := reg.ResolveEndpoint(providerID, proto, modelID, endpointOverride, false)
	if err != nil {
		return nil, err
	}
	headers := cloneHeaders(entry.DefaultHeaders)
	if modelID != "" {
		if ov, ok := entry.ModelOverrides[modelID]; ok {
			for k, v := range ov.DefaultHeaders {
				headers[k] = v
			}
		}
	}
	cfg := RequestConfig{BaseURL: base, APIKey: apiKey, Headers: headers}
	switch proto {
	case ProtocolOpenAIChatCompletions:
		return c.OpenAIChatCompletions(ctx, cfg, payload)
	case ProtocolOpenAIResponses:
		return c.OpenAIResponses(ctx, cfg, payload)
	case ProtocolAnthropicMessages:
		return c.AnthropicMessages(ctx, cfg, payload)
	case ProtocolGoogleGemini:
		if modelID == "" {
			return nil, fmt.Errorf("google_gemini requires modelID")
		}
		return c.GoogleGeminiGenerate(ctx, cfg, modelID, payload)
	default:
		return nil, fmt.Errorf("unsupported protocol %q", proto)
	}
}

func (c *Client) postJSON(ctx context.Context, rawURL string, headers map[string]string, payload map[string]any, allowInsecure bool) (*JSONResponse, error) {
	if err := ValidateEndpointURL(rawURL, allowInsecure); err != nil {
		return nil, err
	}
	encoded, err := json.Marshal(payload)
	if err != nil {
		return nil, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, rawURL, bytes.NewReader(encoded))
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	resp, err := c.HTTP.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, err
	}
	if resp.StatusCode < 200 || resp.StatusCode > 299 {
		return nil, &HTTPStatusError{Status: resp.StatusCode, Headers: resp.Header, Body: raw}
	}
	var body map[string]any
	if err := json.Unmarshal(raw, &body); err != nil {
		return nil, err
	}
	return &JSONResponse{Status: resp.StatusCode, Headers: resp.Header, RawBytes: raw, Body: body}, nil
}

func (c *Client) postSSE(ctx context.Context, rawURL string, headers map[string]string, payload map[string]any, allowInsecure bool) ([]SSEEvent, error) {
	if err := ValidateEndpointURL(rawURL, allowInsecure); err != nil {
		return nil, err
	}
	encoded, err := json.Marshal(payload)
	if err != nil {
		return nil, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, rawURL, bytes.NewReader(encoded))
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "text/event-stream")
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	resp, err := c.HTTP.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode > 299 {
		raw, _ := io.ReadAll(resp.Body)
		return nil, &HTTPStatusError{Status: resp.StatusCode, Headers: resp.Header, Body: raw}
	}
	return ReadSSE(resp.Body)
}

func joinAnthropicMessages(base string) string {
	trimmed := strings.TrimRight(base, "/")
	if strings.HasSuffix(trimmed, "/v1") {
		return trimmed + "/messages"
	}
	return trimmed + "/v1/messages"
}

func cloneHeaders(in map[string]string) map[string]string {
	out := make(map[string]string, len(in))
	for k, v := range in {
		out[k] = v
	}
	return out
}

func withStreamTrue(payload map[string]any) map[string]any {
	out := make(map[string]any, len(payload)+1)
	for k, v := range payload {
		out[k] = v
	}
	out["stream"] = true
	return out
}

func applyBearer(headers map[string]string, key string) error {
	if key == "" {
		return fmt.Errorf("bearer API key is required")
	}
	if _, ok := headers["Authorization"]; !ok {
		headers["Authorization"] = "Bearer " + key
	}
	return nil
}

func applyHeaderAuth(headers map[string]string, name, key string) error {
	if key == "" {
		return fmt.Errorf("%s API key is required", name)
	}
	if _, ok := headers[name]; !ok {
		headers[name] = key
	}
	return nil
}
