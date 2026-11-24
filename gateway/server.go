package main

import (
	"crypto/subtle"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"

	"github.com/LingXiFox/LingXiModelSDK/sdk/go/native"
	"github.com/LingXiFox/LingXiModelSDK/sdk/go/unified"
)

const maxGatewayBodyBytes = 16 << 20

type GatewayConfig struct {
	ListenAddr          string
	AuthToken           string
	AllowHosts          []string
	AllowLoopbackEgress bool
	RateLimitRPS        float64
	RateLimitBurst      float64
}

type Gateway struct {
	cfg      GatewayConfig
	registry *native.Registry
	unified  *unified.Client
	limiter  *TokenBucketLimiter
}

func NewGateway(cfg GatewayConfig, registry *native.Registry) *Gateway {
	cfg.AllowLoopbackEgress = cfg.AllowLoopbackEgress && isLoopbackAddr(cfg.ListenAddr)
	u := unified.NewClient()
	u.Native.HTTP = NewSafeHTTPClient(cfg.AllowLoopbackEgress)
	return &Gateway{
		cfg:      cfg,
		registry: registry,
		unified:  u,
		limiter:  NewTokenBucketLimiter(cfg.RateLimitRPS, cfg.RateLimitBurst),
	}
}

type gatewayCompleteReq struct {
	ProviderID       string          `json:"providerId"`
	Protocol         native.Protocol `json:"protocol"`
	EndpointOverride string          `json:"endpointOverride,omitempty"`
	Request          unified.Request `json:"request"`
}

type gatewayEmbedReq struct {
	ProviderID       string                   `json:"providerId"`
	Protocol         native.Protocol          `json:"protocol"`
	EndpointOverride string                   `json:"endpointOverride,omitempty"`
	Request          unified.EmbeddingRequest `json:"request"`
}

type gatewayRerankReq struct {
	ProviderID       string                `json:"providerId"`
	Protocol         native.Protocol       `json:"protocol"`
	EndpointOverride string                `json:"endpointOverride,omitempty"`
	Request          unified.RerankRequest `json:"request"`
}

type diagnosticsReq struct {
	ProviderID       string          `json:"providerId"`
	Protocol         native.Protocol `json:"protocol"`
	ModelID          string          `json:"modelId,omitempty"`
	EndpointOverride string          `json:"endpointOverride,omitempty"`
}

func (g *Gateway) Handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /v1/healthz", g.handleHealthz)
	mux.HandleFunc("POST /v1/lmp/complete", g.withPolicy(g.handleComplete))
	mux.HandleFunc("POST /v1/lmp/stream", g.withPolicy(g.handleStream))
	mux.HandleFunc("POST /v1/lmp/embed", g.withPolicy(g.handleEmbed))
	mux.HandleFunc("POST /v1/lmp/rerank", g.withPolicy(g.handleRerank))
	mux.HandleFunc("POST /v1/diagnostics/connect", g.withPolicy(g.handleDiagnostics))

	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("X-Content-Type-Options", "nosniff")
		w.Header().Set("X-Frame-Options", "DENY")
		w.Header().Set("Cache-Control", "no-store")
		mux.ServeHTTP(w, r)
	})
}

func (g *Gateway) withPolicy(next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if g.cfg.AuthToken != "" {
			expected := "Bearer " + g.cfg.AuthToken
			if subtle.ConstantTimeCompare([]byte(r.Header.Get("Authorization")), []byte(expected)) != 1 {
				writeErrorJSON(w, http.StatusUnauthorized, "unauthorized", "missing or invalid gateway bearer token")
				return
			}
		}
		if !g.limiter.Allow(clientKey(r)) {
			writeErrorJSON(w, http.StatusTooManyRequests, "rate_limited", "gateway rate limit exceeded")
			return
		}
		next(w, r)
	}
}

func (g *Gateway) handleHealthz(w http.ResponseWriter, r *http.Request) {
	mode := "remote"
	if isLoopbackAddr(g.cfg.ListenAddr) {
		mode = "loopback"
	}
	regCount := 0
	if g.registry != nil {
		regCount = g.registry.Len()
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"status":            "ok",
		"version":           "v1",
		"mode":              mode,
		"registryProviders": regCount,
	})
}

func (g *Gateway) resolveTarget(providerID string, proto native.Protocol, modelID, endpointOverride string) (string, error) {
	var target string
	if providerID != "" && g.registry != nil {
		_, resolved, err := g.registry.ResolveEndpoint(
			providerID,
			proto,
			modelID,
			endpointOverride,
			g.cfg.AllowLoopbackEgress,
		)
		if err != nil {
			return "", err
		}
		target = resolved
	} else {
		if endpointOverride == "" {
			return "", errors.New("providerId (with registry) or endpointOverride is required")
		}
		if err := native.ValidateEndpointURL(endpointOverride, g.cfg.AllowLoopbackEgress); err != nil {
			return "", err
		}
		target = strings.TrimRight(endpointOverride, "/")
	}
	if err := CheckEgressAllowed(target, g.cfg.AllowHosts, g.cfg.AllowLoopbackEgress); err != nil {
		return "", err
	}
	return target, nil
}

func (g *Gateway) handleComplete(w http.ResponseWriter, r *http.Request) {
	var req gatewayCompleteReq
	if err := decodeBoundedJSON(r.Body, &req); err != nil {
		writeErrorJSON(w, http.StatusBadRequest, "invalid_request", err.Error())
		return
	}
	apiKey := r.Header.Get("X-Provider-API-Key")
	if apiKey == "" && req.Protocol != native.ProtocolOllama {
		writeErrorJSON(w, http.StatusBadRequest, "auth_missing", "X-Provider-API-Key header is required")
		return
	}
	target, err := g.resolveTarget(req.ProviderID, req.Protocol, req.Request.Model, req.EndpointOverride)
	if err != nil {
		writeErrorJSON(w, http.StatusForbidden, "egress_or_registry_error", "provider endpoint or protocol rejected")
		return
	}

	resp, err := g.unified.Complete(r.Context(), req.Protocol, target, apiKey, req.Request)
	if err != nil {
		writeLmpError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, resp)
}

func (g *Gateway) handleStream(w http.ResponseWriter, r *http.Request) {
	var req gatewayCompleteReq
	if err := decodeBoundedJSON(r.Body, &req); err != nil {
		writeErrorJSON(w, http.StatusBadRequest, "invalid_request", err.Error())
		return
	}
	apiKey := r.Header.Get("X-Provider-API-Key")
	if apiKey == "" && req.Protocol != native.ProtocolOllama {
		writeErrorJSON(w, http.StatusBadRequest, "auth_missing", "X-Provider-API-Key header is required")
		return
	}
	target, err := g.resolveTarget(req.ProviderID, req.Protocol, req.Request.Model, req.EndpointOverride)
	if err != nil {
		writeErrorJSON(w, http.StatusForbidden, "egress_or_registry_error", "provider endpoint or protocol rejected")
		return
	}

	w.Header().Set("Content-Type", "text/event-stream; charset=utf-8")
	w.Header().Set("X-Accel-Buffering", "no")
	flusher, _ := w.(http.Flusher)

	err = g.unified.StreamEach(r.Context(), req.Protocol, target, apiKey, req.Request, func(ev unified.StreamEvent) error {
		select {
		case <-r.Context().Done():
			return r.Context().Err()
		default:
		}
		data, err := json.Marshal(ev)
		if err != nil {
			return err
		}
		if _, err := fmt.Fprintf(w, "event: %s\ndata: %s\n\n", ev.Type, data); err != nil {
			return err
		}
		if flusher != nil {
			flusher.Flush()
		}
		return nil
	})
	if err != nil {
		if r.Context().Err() != nil {
			return
		}
		_, _, message := lmpErrorResponse(err)
		errPayload, _ := json.Marshal(map[string]string{"error": message})
		_, _ = fmt.Fprintf(w, "event: error\ndata: %s\n\n", errPayload)
		if flusher != nil {
			flusher.Flush()
		}
		return
	}
	_, _ = fmt.Fprint(w, "data: [DONE]\n\n")
	if flusher != nil {
		flusher.Flush()
	}
}

func (g *Gateway) handleEmbed(w http.ResponseWriter, r *http.Request) {
	var req gatewayEmbedReq
	if err := decodeBoundedJSON(r.Body, &req); err != nil {
		writeErrorJSON(w, http.StatusBadRequest, "invalid_request", err.Error())
		return
	}
	apiKey := r.Header.Get("X-Provider-API-Key")
	if apiKey == "" && req.Protocol != native.ProtocolOllama {
		writeErrorJSON(w, http.StatusBadRequest, "auth_missing", "X-Provider-API-Key header is required")
		return
	}
	target, err := g.resolveTarget(req.ProviderID, req.Protocol, req.Request.Model, req.EndpointOverride)
	if err != nil {
		writeErrorJSON(w, http.StatusForbidden, "egress_or_registry_error", "provider endpoint or protocol rejected")
		return
	}
	resp, err := g.unified.Embed(r.Context(), req.Protocol, target, apiKey, req.Request)
	if err != nil {
		writeLmpError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, resp)
}

func (g *Gateway) handleRerank(w http.ResponseWriter, r *http.Request) {
	var req gatewayRerankReq
	if err := decodeBoundedJSON(r.Body, &req); err != nil {
		writeErrorJSON(w, http.StatusBadRequest, "invalid_request", err.Error())
		return
	}
	apiKey := r.Header.Get("X-Provider-API-Key")
	if apiKey == "" {
		writeErrorJSON(w, http.StatusBadRequest, "auth_missing", "X-Provider-API-Key header is required")
		return
	}
	target, err := g.resolveTarget(req.ProviderID, req.Protocol, req.Request.Model, req.EndpointOverride)
	if err != nil {
		writeErrorJSON(w, http.StatusForbidden, "egress_or_registry_error", "provider endpoint or protocol rejected")
		return
	}
	resp, err := g.unified.Rerank(r.Context(), req.Protocol, target, apiKey, req.Request)
	if err != nil {
		writeLmpError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, resp)
}

func (g *Gateway) handleDiagnostics(w http.ResponseWriter, r *http.Request) {
	var req diagnosticsReq
	if err := decodeBoundedJSON(r.Body, &req); err != nil {
		writeErrorJSON(w, http.StatusBadRequest, "invalid_request", err.Error())
		return
	}
	target, err := g.resolveTarget(req.ProviderID, req.Protocol, req.ModelID, req.EndpointOverride)
	if err != nil {
		writeErrorJSON(w, http.StatusForbidden, "diagnostics_failed", "provider endpoint or protocol rejected")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"ok":       true,
		"protocol": req.Protocol,
		"endpoint": target,
	})
}

func decodeBoundedJSON(body io.Reader, dst any) error {
	raw, err := io.ReadAll(io.LimitReader(body, maxGatewayBodyBytes+1))
	if err != nil {
		return err
	}
	if len(raw) > maxGatewayBodyBytes {
		return errors.New("request body exceeds 16 MiB limit")
	}
	return json.Unmarshal(raw, dst)
}

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}

func writeErrorJSON(w http.ResponseWriter, status int, code, msg string) {
	writeJSON(w, status, map[string]any{
		"error": map[string]any{
			"code":    code,
			"message": msg,
		},
	})
}

func writeLmpError(w http.ResponseWriter, err error) {
	status, code, message := lmpErrorResponse(err)
	writeErrorJSON(w, status, code, message)
}

func lmpErrorResponse(err error) (int, string, string) {
	var unsup *unified.UnsupportedFeatureError
	if errors.As(err, &unsup) {
		return http.StatusUnprocessableEntity, "unsupported_feature", unsup.Error()
	}
	var httpErr *native.HTTPStatusError
	if errors.As(err, &httpErr) {
		status := httpErr.Status
		if status < 400 {
			status = http.StatusBadGateway
		}
		return status, "upstream_error", fmt.Sprintf("upstream returned HTTP %d", httpErr.Status)
	}
	return http.StatusBadGateway, "gateway_error", "upstream request failed"
}
