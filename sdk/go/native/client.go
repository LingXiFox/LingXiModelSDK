package native

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

const maxNativeResponseBytes = 32 << 20

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

// NewClient constructs a Native Client with redirect blocking enabled.
func NewClient() *Client {
	return &Client{HTTP: &http.Client{
		Timeout: 60 * time.Second,
		CheckRedirect: func(*http.Request, []*http.Request) error {
			return http.ErrUseLastResponse
		},
	}}
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

func (c *Client) AWSBedrockConverse(ctx context.Context, cfg RequestConfig, model string, payload map[string]any) (*JSONResponse, error) {
	uri := fmt.Sprintf("/model/%s/converse", awsEncodePath(model, false))
	rawURL := strings.TrimRight(cfg.BaseURL, "/") + uri
	headers := cloneHeaders(cfg.Headers)
	if err := applyBedrockAuth(rawURL, payload, cfg.APIKey, headers); err != nil {
		return nil, err
	}
	return c.postJSON(ctx, rawURL, headers, payload, cfg.AllowInsecureHTTP)
}

func (c *Client) StreamAWSBedrockConverseEach(ctx context.Context, cfg RequestConfig, model string, payload map[string]any, fn func(SSEEvent) error) error {
	uri := fmt.Sprintf("/model/%s/converse-stream", awsEncodePath(model, false))
	rawURL := strings.TrimRight(cfg.BaseURL, "/") + uri
	headers := cloneHeaders(cfg.Headers)
	if err := applyBedrockAuth(rawURL, payload, cfg.APIKey, headers); err != nil {
		return err
	}
	return c.postStreamEach(ctx, rawURL, amazonEventStream, headers, payload, cfg.AllowInsecureHTTP, func(r io.Reader, contentType string) error {
		switch strings.ToLower(strings.TrimSpace(strings.SplitN(contentType, ";", 2)[0])) {
		case amazonEventStream:
			return readAmazonEvents(r, fn)
		case "application/x-ndjson":
			return ReadNDJSONEach(r, fn)
		default:
			return fmt.Errorf("unexpected Bedrock response content type")
		}
	})
}

func (c *Client) GoogleVertexGenerate(ctx context.Context, cfg RequestConfig, model string, payload map[string]any) (*JSONResponse, error) {
	base := strings.TrimRight(cfg.BaseURL, "/")
	rawURL := base + "/v1/publishers/google/models/" + model + ":generateContent"
	if strings.Contains(base, "/models") {
		rawURL = base + "/" + model + ":generateContent"
	}
	headers := cloneHeaders(cfg.Headers)
	if err := applyBearer(headers, cfg.APIKey); err != nil {
		return nil, err
	}
	return c.postJSON(ctx, rawURL, headers, payload, cfg.AllowInsecureHTTP)
}

func (c *Client) StreamGoogleVertexGenerateEach(ctx context.Context, cfg RequestConfig, model string, payload map[string]any, fn func(SSEEvent) error) error {
	base := strings.TrimRight(cfg.BaseURL, "/")
	rawURL := base + "/v1/publishers/google/models/" + model + ":streamGenerateContent?alt=sse"
	if strings.Contains(base, "/models") {
		rawURL = base + "/" + model + ":streamGenerateContent?alt=sse"
	}
	headers := cloneHeaders(cfg.Headers)
	if err := applyBearer(headers, cfg.APIKey); err != nil {
		return err
	}
	return c.postSSEEach(ctx, rawURL, headers, payload, cfg.AllowInsecureHTTP, fn)
}

func (c *Client) AzureOpenAIChatCompletions(ctx context.Context, cfg RequestConfig, deployment string, payload map[string]any) (*JSONResponse, error) {
	apiVer := cfg.Headers["x-azure-api-version"]
	if apiVer == "" {
		apiVer = "2024-10-21"
	}
	rawURL := fmt.Sprintf("%s/openai/deployments/%s/chat/completions?api-version=%s", strings.TrimRight(cfg.BaseURL, "/"), deployment, apiVer)
	headers := cloneHeaders(cfg.Headers)
	if err := applyHeaderAuth(headers, "api-key", cfg.APIKey); err != nil {
		return nil, err
	}
	return c.postJSON(ctx, rawURL, headers, payload, cfg.AllowInsecureHTTP)
}

func (c *Client) StreamAzureOpenAIChatCompletionsEach(ctx context.Context, cfg RequestConfig, deployment string, payload map[string]any, fn func(SSEEvent) error) error {
	apiVer := cfg.Headers["x-azure-api-version"]
	if apiVer == "" {
		apiVer = "2024-10-21"
	}
	rawURL := fmt.Sprintf("%s/openai/deployments/%s/chat/completions?api-version=%s", strings.TrimRight(cfg.BaseURL, "/"), deployment, apiVer)
	headers := cloneHeaders(cfg.Headers)
	if err := applyHeaderAuth(headers, "api-key", cfg.APIKey); err != nil {
		return err
	}
	return c.postSSEEach(ctx, rawURL, headers, withStreamTrue(payload), cfg.AllowInsecureHTTP, fn)
}

func (c *Client) OllamaChat(ctx context.Context, cfg RequestConfig, payload map[string]any) (*JSONResponse, error) {
	rawURL := strings.TrimRight(cfg.BaseURL, "/") + "/api/chat"
	body := cloneAnyMap(payload)
	if _, ok := body["stream"]; !ok {
		body["stream"] = false
	}
	return c.postJSON(ctx, rawURL, cfg.Headers, body, cfg.AllowInsecureHTTP)
}

func (c *Client) StreamOllamaChatEach(ctx context.Context, cfg RequestConfig, payload map[string]any, fn func(SSEEvent) error) error {
	rawURL := strings.TrimRight(cfg.BaseURL, "/") + "/api/chat"
	return c.postNDJSONEach(ctx, rawURL, cfg.Headers, withStreamTrue(payload), cfg.AllowInsecureHTTP, fn)
}

func (c *Client) CohereChat(ctx context.Context, cfg RequestConfig, payload map[string]any) (*JSONResponse, error) {
	rawURL := strings.TrimRight(cfg.BaseURL, "/") + "/v2/chat"
	headers := cloneHeaders(cfg.Headers)
	if err := applyBearer(headers, cfg.APIKey); err != nil {
		return nil, err
	}
	return c.postJSON(ctx, rawURL, headers, payload, cfg.AllowInsecureHTTP)
}

func (c *Client) StreamCohereChatEach(ctx context.Context, cfg RequestConfig, payload map[string]any, fn func(SSEEvent) error) error {
	rawURL := strings.TrimRight(cfg.BaseURL, "/") + "/v2/chat"
	headers := cloneHeaders(cfg.Headers)
	if err := applyBearer(headers, cfg.APIKey); err != nil {
		return err
	}
	return c.postSSEEach(ctx, rawURL, headers, withStreamTrue(payload), cfg.AllowInsecureHTTP, fn)
}

// StreamOpenAIChatCompletionsEach streams SSE events one by one.
func (c *Client) StreamOpenAIChatCompletionsEach(ctx context.Context, cfg RequestConfig, payload map[string]any, fn func(SSEEvent) error) error {
	rawURL := strings.TrimRight(cfg.BaseURL, "/") + "/chat/completions"
	headers := cloneHeaders(cfg.Headers)
	if err := applyBearer(headers, cfg.APIKey); err != nil {
		return err
	}
	return c.postSSEEach(ctx, rawURL, headers, withStreamTrue(payload), cfg.AllowInsecureHTTP, fn)
}

func (c *Client) StreamOpenAIResponsesEach(ctx context.Context, cfg RequestConfig, payload map[string]any, fn func(SSEEvent) error) error {
	rawURL := strings.TrimRight(cfg.BaseURL, "/") + "/responses"
	headers := cloneHeaders(cfg.Headers)
	if err := applyBearer(headers, cfg.APIKey); err != nil {
		return err
	}
	return c.postSSEEach(ctx, rawURL, headers, withStreamTrue(payload), cfg.AllowInsecureHTTP, fn)
}

func (c *Client) StreamAnthropicMessagesEach(ctx context.Context, cfg RequestConfig, payload map[string]any, fn func(SSEEvent) error) error {
	rawURL := joinAnthropicMessages(cfg.BaseURL)
	headers := cloneHeaders(cfg.Headers)
	if err := applyHeaderAuth(headers, "x-api-key", cfg.APIKey); err != nil {
		return err
	}
	if _, ok := headers["anthropic-version"]; !ok {
		headers["anthropic-version"] = "2023-06-01"
	}
	return c.postSSEEach(ctx, rawURL, headers, withStreamTrue(payload), cfg.AllowInsecureHTTP, fn)
}

func (c *Client) StreamGoogleGeminiGenerateEach(ctx context.Context, cfg RequestConfig, model string, payload map[string]any, fn func(SSEEvent) error) error {
	rawURL := fmt.Sprintf("%s/v1beta/models/%s:streamGenerateContent?alt=sse", strings.TrimRight(cfg.BaseURL, "/"), model)
	headers := cloneHeaders(cfg.Headers)
	if err := applyHeaderAuth(headers, "x-goog-api-key", cfg.APIKey); err != nil {
		return err
	}
	return c.postSSEEach(ctx, rawURL, headers, payload, cfg.AllowInsecureHTTP, fn)
}

func (c *Client) PostJSONEndpoint(ctx context.Context, rawURL string, headers map[string]string, payload map[string]any, allowInsecure bool) (*JSONResponse, error) {
	return c.postJSON(ctx, rawURL, headers, payload, allowInsecure)
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
	case ProtocolAWSBedrock:
		if modelID == "" {
			return nil, fmt.Errorf("aws_bedrock requires modelID")
		}
		return c.AWSBedrockConverse(ctx, cfg, modelID, payload)
	case ProtocolGoogleVertexAI:
		if modelID == "" {
			return nil, fmt.Errorf("google_vertex_ai requires modelID")
		}
		return c.GoogleVertexGenerate(ctx, cfg, modelID, payload)
	case ProtocolAzureOpenAI:
		if modelID == "" {
			return nil, fmt.Errorf("azure_openai requires modelID/deployment")
		}
		return c.AzureOpenAIChatCompletions(ctx, cfg, modelID, payload)
	case ProtocolOllama:
		return c.OllamaChat(ctx, cfg, payload)
	case ProtocolCohere:
		return c.CohereChat(ctx, cfg, payload)
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
	raw, err := io.ReadAll(io.LimitReader(resp.Body, maxNativeResponseBytes+1))
	if err != nil {
		return nil, err
	}
	if len(raw) > maxNativeResponseBytes {
		return nil, fmt.Errorf("upstream response exceeded %d bytes", maxNativeResponseBytes)
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
	var events []SSEEvent
	err := c.postSSEEach(ctx, rawURL, headers, payload, allowInsecure, func(ev SSEEvent) error {
		events = append(events, ev)
		return nil
	})
	return events, err
}

func (c *Client) postSSEEach(ctx context.Context, rawURL string, headers map[string]string, payload map[string]any, allowInsecure bool, fn func(SSEEvent) error) error {
	return c.postStreamEach(ctx, rawURL, "text/event-stream", headers, payload, allowInsecure, func(r io.Reader, _ string) error {
		return ReadSSEEach(r, fn)
	})
}

func (c *Client) postNDJSONEach(ctx context.Context, rawURL string, headers map[string]string, payload map[string]any, allowInsecure bool, fn func(SSEEvent) error) error {
	return c.postStreamEach(ctx, rawURL, "application/x-ndjson, application/json", headers, payload, allowInsecure, func(r io.Reader, _ string) error {
		return ReadNDJSONEach(r, fn)
	})
}

func (c *Client) postStreamEach(ctx context.Context, rawURL, accept string, headers map[string]string, payload map[string]any, allowInsecure bool, parse func(io.Reader, string) error) error {
	if err := ValidateEndpointURL(rawURL, allowInsecure); err != nil {
		return err
	}
	encoded, err := json.Marshal(payload)
	if err != nil {
		return err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, rawURL, bytes.NewReader(encoded))
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", accept)
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	resp, err := c.HTTP.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode > 299 {
		raw, _ := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
		return &HTTPStatusError{Status: resp.StatusCode, Headers: resp.Header, Body: raw}
	}
	limited := &io.LimitedReader{R: resp.Body, N: maxNativeResponseBytes + 1}
	err = parse(limited, resp.Header.Get("Content-Type"))
	if limited.N == 0 {
		return fmt.Errorf("upstream stream exceeded %d bytes", maxNativeResponseBytes)
	}
	return err
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

func cloneAnyMap(in map[string]any) map[string]any {
	out := make(map[string]any, len(in))
	for k, v := range in {
		out[k] = v
	}
	return out
}

func awsEncodePath(value string, preserveSlash bool) string {
	var out strings.Builder
	for i := 0; i < len(value); i++ {
		b := value[i]
		if b >= 'a' && b <= 'z' || b >= 'A' && b <= 'Z' || b >= '0' && b <= '9' ||
			strings.IndexByte("-._~", b) >= 0 || preserveSlash && b == '/' {
			out.WriteByte(b)
		} else {
			fmt.Fprintf(&out, "%%%02X", b)
		}
	}
	return out.String()
}

func applyBedrockAuth(rawURL string, payload map[string]any, apiKey string, headers map[string]string) error {
	if _, ok := headers["Authorization"]; ok {
		return nil
	}
	if apiKey == "" {
		return fmt.Errorf("AWS Bedrock requires 'ACCESS_KEY:SECRET_KEY[:SESSION_TOKEN]'")
	}
	parts := strings.Split(apiKey, ":")
	if len(parts) < 2 {
		return applyBearer(headers, apiKey)
	}
	u, err := url.Parse(rawURL)
	if err != nil {
		return err
	}
	region := headers["x-aws-region"]
	if region == "" {
		if tail, ok := strings.CutPrefix(u.Hostname(), "bedrock-runtime."); ok {
			candidate, domain, _ := strings.Cut(tail, ".")
			if domain == "amazonaws.com" || domain == "amazonaws.com.cn" || domain == "api.aws" {
				region = candidate
			}
		}
		if region == "" {
			region = "us-east-1"
		}
	}
	session := ""
	if len(parts) >= 3 {
		session = parts[2]
	}
	encoded, err := json.Marshal(payload)
	if err != nil {
		return err
	}
	for k, v := range SignSigV4("POST", awsEncodePath(u.EscapedPath(), true), "", u.Host, region, "bedrock", parts[0], parts[1], session, encoded, time.Now()) {
		headers[k] = v
	}
	return nil
}
