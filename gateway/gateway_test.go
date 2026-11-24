package main

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/LingXiFox/LingXiModelSDK/sdk/go/native"
)

func loadTestRegistry(t *testing.T) *native.Registry {
	t.Helper()
	b, err := os.ReadFile(filepath.Join("..", "registry", "registry.json"))
	if err != nil {
		t.Fatal(err)
	}
	reg, err := native.ParseRegistry(b)
	if err != nil {
		t.Fatal(err)
	}
	return reg
}

func TestGatewaySecurityPolicies(t *testing.T) {
	if err := ValidateStartupSecurity("0.0.0.0:18090", "", nil); err == nil {
		t.Fatal("expected remote bind without auth to fail")
	}
	if err := ValidateStartupSecurity("0.0.0.0:18090", "tok", nil); err == nil {
		t.Fatal("expected remote bind without allowlist to fail")
	}
	if err := ValidateStartupSecurity("0.0.0.0:18090", "tok", []string{"api.openai.com"}); err != nil {
		t.Fatalf("expected valid remote config to pass: %v", err)
	}
	if err := ValidateStartupSecurity("127.0.0.1:18090", "", nil); err != nil {
		t.Fatalf("expected loopback config to pass: %v", err)
	}
	if err := CheckEgressAllowed("https://evil.internal/v1", []string{"api.openai.com"}, false); err == nil {
		t.Fatal("expected SSRF host to be blocked")
	}
}

func TestGatewayEndpointsCompleteStreamAuthRateLimit(t *testing.T) {
	chatResp, err := os.ReadFile(filepath.Join("..", "conformance", "native", "openai_chat_response.json"))
	if err != nil {
		t.Fatal(err)
	}
	chatSSE, err := os.ReadFile(filepath.Join("..", "conformance", "native", "openai_chat_stream.sse"))
	if err != nil {
		t.Fatal(err)
	}
	var isSSE bool
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if isSSE {
			w.Header().Set("Content-Type", "text/event-stream")
			_, _ = w.Write(chatSSE)
			return
		}
		_, _ = w.Write(chatResp)
	}))
	defer upstream.Close()

	gw := NewGateway(GatewayConfig{
		ListenAddr:          "127.0.0.1:18090",
		AuthToken:           "gw-secret",
		AllowLoopbackEgress: true,
		RateLimitRPS:        100,
		RateLimitBurst:      3,
	}, loadTestRegistry(t))
	h := gw.Handler()

	// 1. Healthz
	w := httptest.NewRecorder()
	h.ServeHTTP(w, httptest.NewRequest(http.MethodGet, "/v1/healthz", nil))
	if w.Code != http.StatusOK || !strings.Contains(w.Body.String(), `"status":"ok"`) {
		t.Fatalf("healthz failed: %d %s", w.Code, w.Body.String())
	}

	// 2. Unauthorized without bearer token
	w = httptest.NewRecorder()
	h.ServeHTTP(w, httptest.NewRequest(http.MethodPost, "/v1/diagnostics/connect", strings.NewReader(`{}`)))
	if w.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", w.Code)
	}

	// 3. Complete via registry + loopback override
	body := `{"providerId":"openai","protocol":"openai_chat_completions","endpointOverride":"` + upstream.URL + `","request":{"model":"gpt-5-mini","messages":[{"role":"user","content":[{"type":"text","text":"hi"}]}]}}`
	req := httptest.NewRequest(http.MethodPost, "/v1/lmp/complete", strings.NewReader(body))
	req.Header.Set("Authorization", "Bearer gw-secret")
	req.Header.Set("X-Provider-API-Key", "sk-test")
	w = httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("complete failed: %d %s", w.Code, w.Body.String())
	}
	var parsed map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &parsed); err != nil || parsed["finishReason"] != "stop" {
		t.Fatalf("unexpected complete response: %s", w.Body.String())
	}

	// 4. Stream via SSE
	isSSE = true
	req = httptest.NewRequest(http.MethodPost, "/v1/lmp/stream", bytes.NewBufferString(body))
	req.Header.Set("Authorization", "Bearer gw-secret")
	req.Header.Set("X-Provider-API-Key", "sk-test")
	w = httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusOK || !strings.Contains(w.Body.String(), "event: text_delta") || !strings.Contains(w.Body.String(), "data: [DONE]") {
		t.Fatalf("stream failed: %d %s", w.Code, w.Body.String())
	}

	// 5. Diagnostics endpoint
	req = httptest.NewRequest(http.MethodPost, "/v1/diagnostics/connect", strings.NewReader(`{"providerId":"anthropic","protocol":"anthropic_messages"}`))
	req.Header.Set("Authorization", "Bearer gw-secret")
	w = httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusOK || !strings.Contains(w.Body.String(), "https://api.anthropic.com") {
		t.Fatalf("diagnostics failed: %d %s", w.Code, w.Body.String())
	}

	// 6. Rate limiter triggers 429 after burst=3
	req = httptest.NewRequest(http.MethodPost, "/v1/diagnostics/connect", strings.NewReader(`{"providerId":"openai","protocol":"openai_chat_completions"}`))
	req.Header.Set("Authorization", "Bearer gw-secret")
	w = httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusTooManyRequests {
		t.Fatalf("expected 429 on 4th burst request, got %d", w.Code)
	}
}

func TestGatewaySSRFPrivateIPBlockAndEmbedRerank(t *testing.T) {
	if err := CheckEgressAllowed("https://169.254.169.254/latest/meta-data", []string{"169.254.169.254"}, false); err == nil {
		t.Fatalf("expected metadata IP 169.254.169.254 to be blocked even if in allowlist")
	}
	if err := CheckEgressAllowed("https://10.0.0.5/v1", []string{"10.0.0.5"}, false); err == nil {
		t.Fatalf("expected RFC1918 IP 10.0.0.5 to be blocked")
	}
	if got := RedactSecret("sk-ant-1234567890abcdef"); got != "sk-a...ef" {
		t.Fatalf("unexpected redact result: %q", got)
	}

	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/embeddings":
			_, _ = w.Write([]byte(`{"object":"list","data":[{"index":0,"embedding":[0.1,0.2]}]}`))
		case "/v2/rerank":
			_, _ = w.Write([]byte(`{"id":"rr","results":[{"index":0,"relevance_score":0.9}]}`))
		}
	}))
	defer upstream.Close()

	gw := NewGateway(GatewayConfig{
		ListenAddr:          "127.0.0.1:0",
		AllowLoopbackEgress: true,
		RateLimitRPS:        50,
		RateLimitBurst:      50,
	}, nil)
	h := gw.Handler()

	embReq := httptest.NewRequest(http.MethodPost, "/v1/lmp/embed", strings.NewReader(`{"protocol":"openai_chat_completions","endpointOverride":"`+upstream.URL+`","request":{"model":"text-embedding-3-small","input":["hi"]}}`))
	embReq.Header.Set("X-Provider-API-Key", "sk-test")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, embReq)
	if w.Code != http.StatusOK || !strings.Contains(w.Body.String(), "0.2") {
		t.Fatalf("gateway embed failed: %d %s", w.Code, w.Body.String())
	}

	rrReq := httptest.NewRequest(http.MethodPost, "/v1/lmp/rerank", strings.NewReader(`{"protocol":"cohere","endpointOverride":"`+upstream.URL+`","request":{"model":"rerank-v3.5","query":"q","documents":["d"]}}`))
	rrReq.Header.Set("X-Provider-API-Key", "co-test")
	w = httptest.NewRecorder()
	h.ServeHTTP(w, rrReq)
	if w.Code != http.StatusOK || !strings.Contains(w.Body.String(), "0.9") {
		t.Fatalf("gateway rerank failed: %d %s", w.Code, w.Body.String())
	}
}
