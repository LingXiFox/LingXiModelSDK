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
	return &Gateway{
		cfg:      cfg,
		registry: registry,
		unified:  unified.NewClient(),
		limiter:  NewTokenBucketLimiter(cfg.RateLimitRPS, cfg.RateLimitBurst),
	}
}

type gatewayCompleteReq struct {
	ProviderID       string          `json:"providerId"`
	Protocol         native.Protocol `json:"protocol"`
	EndpointOverride string          `json:"endpointOverride,omitempty"`
	Request          unified.Request `json:"request"`
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
	if apiKey == "" {
		writeErrorJSON(w, http.StatusBadRequest, "auth_missing", "X-Provider-API-Key header is required")
		return
	}
	target, err := g.resolveTarget(req.ProviderID, req.Protocol, req.Request.Model, req.EndpointOverride)
	if err != nil {
		writeErrorJSON(w, http.StatusForbidden, "egress_or_registry_error", err.Error())
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
	if apiKey == "" {
		writeErrorJSON(w, http.StatusBadRequest, "auth_missing", "X-Provider-API-Key header is required")
		return
	}
	target, err := g.resolveTarget(req.ProviderID, req.Protocol, req.Request.Model, req.EndpointOverride)
	if err != nil {
		writeErrorJSON(w, http.StatusForbidden, "egress_or_registry_error", err.Error())
		return
	}

	events, err := g.unified.Stream(r.Context(), req.Protocol, target, apiKey, req.Request)
	if err != nil {
		writeLmpError(w, err)
		return
	}

	w.Header().Set("Content-Type", "text/event-stream; charset=utf-8")
	w.Header().Set("X-Accel-Buffering", "no")
	flusher, _ := w.(http.Flusher)

	for _, ev := range events {
		select {
		case <-r.Context().Done():
			return
		default:
		}
		data, err := json.Marshal(ev)
		if err != nil {
			continue
		}
		_, _ = fmt.Fprintf(w, "event: %s\ndata: %s\n\n", ev.Type, data)
		if flusher != nil {
			flusher.Flush()
		}
	}
	_, _ = fmt.Fprint(w, "data: [DONE]\n\n")
	if flusher != nil {
		flusher.Flush()
	}
}

func (g *Gateway) handleDiagnostics(w http.ResponseWriter, r *http.Request) {
	var req diagnosticsReq
	if err := decodeBoundedJSON(r.Body, &req); err != nil {
		writeErrorJSON(w, http.StatusBadRequest, "invalid_request", err.Error())
		return
	}
	target, err := g.resolveTarget(req.ProviderID, req.Protocol, req.ModelID, req.EndpointOverride)
	if err != nil {
		writeErrorJSON(w, http.StatusForbidden, "diagnostics_failed", err.Error())
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
	var unsup *unified.UnsupportedFeatureError
	if errors.As(err, &unsup) {
		writeErrorJSON(w, http.StatusUnprocessableEntity, "unsupported_feature", unsup.Error())
		return
	}
	var httpErr *native.HTTPStatusError
	if errors.As(err, &httpErr) {
		writeErrorJSON(w, httpErr.Status, "upstream_error", httpErr.Error())
		return
	}
	writeErrorJSON(w, http.StatusBadGateway, "gateway_error", err.Error())
}
