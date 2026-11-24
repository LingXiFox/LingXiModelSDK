package main

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestHTTPEndpointsAndConditionalRequests(t *testing.T) {
	store := newStore(t.TempDir(), defaultMinRatio)
	snap := buildTestSnapshot(t, richFixture)
	store.current.Store(snap)
	h := publicHandler(store)

	routes := []string{
		"/",
		"/sdk",
		"/protocols",
		"/providers",
		"/models.json",
		"/publication.json",
		"/healthz",
		"/static/site.css",
		"/static/site.js",
		"/static/beian-icon.png",
		"/beian-icon.png",
	}
	for _, path := range routes {
		w := httptest.NewRecorder()
		h.ServeHTTP(w, httptest.NewRequest(http.MethodGet, path, nil))
		if w.Code != http.StatusOK {
			t.Fatalf("GET %s returned %d: %s", path, w.Code, w.Body.String())
		}
	}

	// /index.html permanently redirects to /.
	w := httptest.NewRecorder()
	h.ServeHTTP(w, httptest.NewRequest(http.MethodGet, "/index.html", nil))
	if w.Code != http.StatusMovedPermanently || w.Header().Get("Location") != "/" {
		t.Fatalf("/index.html redirect failed: %d %q", w.Code, w.Header().Get("Location"))
	}

	// ETag conditional request returns 304.
	req := httptest.NewRequest(http.MethodGet, "/models.json", nil)
	req.Header.Set("If-None-Match", snap.ETag)
	w = httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusNotModified {
		t.Fatalf("expected 304 for matching ETag, got %d", w.Code)
	}

	// If-Modified-Since conditional request returns 304.
	req = httptest.NewRequest(http.MethodGet, "/models.json", nil)
	req.Header.Set("If-Modified-Since", snap.LastModified.Format(http.TimeFormat))
	w = httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusNotModified {
		t.Fatalf("expected 304 for If-Modified-Since, got %d", w.Code)
	}

	// HEAD returns headers with empty body.
	w = httptest.NewRecorder()
	h.ServeHTTP(w, httptest.NewRequest(http.MethodHead, "/models.json", nil))
	if w.Code != http.StatusOK || w.Body.Len() != 0 {
		t.Fatalf("HEAD /models.json failed: %d len=%d", w.Code, w.Body.Len())
	}

	// OPTIONS CORS preflight.
	w = httptest.NewRecorder()
	h.ServeHTTP(w, httptest.NewRequest(http.MethodOptions, "/models.json", nil))
	if w.Code != http.StatusNoContent || w.Header().Get("Access-Control-Allow-Origin") != "*" {
		t.Fatalf("OPTIONS /models.json failed: %d", w.Code)
	}

	// /publication.json matches /models.json sha256.
	w = httptest.NewRecorder()
	h.ServeHTTP(w, httptest.NewRequest(http.MethodGet, "/publication.json", nil))
	var manifest Manifest
	if err := json.Unmarshal(w.Body.Bytes(), &manifest); err != nil || manifest.SHA256 != sha256Hex(snap.Raw) {
		t.Fatalf("publication.json mismatch: %v %+v", err, manifest)
	}

	// /healthz reports JSON status.
	w = httptest.NewRecorder()
	h.ServeHTTP(w, httptest.NewRequest(http.MethodGet, "/healthz", nil))
	var health map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &health); err != nil || health["status"] != "ok" {
		t.Fatalf("unexpected /healthz body: %s", w.Body.String())
	}

	// Filtering & empty query rendering.
	w = httptest.NewRecorder()
	h.ServeHTTP(w, httptest.NewRequest(http.MethodGet, "/?cap=vision", nil))
	if !strings.Contains(w.Body.String(), "GPT-5") || strings.Contains(w.Body.String(), "GPT-10") {
		t.Fatalf("vision filter failed")
	}

	w = httptest.NewRecorder()
	h.ServeHTTP(w, httptest.NewRequest(http.MethodGet, "/?q=no-such-model", nil))
	if !strings.Contains(w.Body.String(), "未找到符合条件的模型") {
		t.Fatalf("empty search message missing")
	}

	// Footer preserves both filing numbers and links.
	w = httptest.NewRecorder()
	h.ServeHTTP(w, httptest.NewRequest(http.MethodGet, "/", nil))
	body := w.Body.String()
	for _, needle := range []string{"苏ICP备2026055395号-1", "苏公网安备32010202012360号", "/static/beian-icon.png"} {
		if !strings.Contains(body, needle) {
			t.Fatalf("footer missing %q", needle)
		}
	}

	// Embedded provider status lookup reflects evidence matrix.
	if st, ok := embeddedProviderStatuses["openai"]; !ok || st.Status != "mock_tested" {
		t.Fatalf("expected openai status=mock_tested, got %+v", st)
	}
	if st, ok := embeddedProviderStatuses["groq"]; !ok || st.Status != "compatible" {
		t.Fatalf("expected groq status=compatible, got %+v", st)
	}
}

func TestHTTPUnavailableAndAdminSync(t *testing.T) {
	store := newStore(t.TempDir(), defaultMinRatio)
	h := publicHandler(store)

	for _, path := range []string{"/", "/models.json", "/publication.json", "/healthz", "/providers"} {
		w := httptest.NewRecorder()
		h.ServeHTTP(w, httptest.NewRequest(http.MethodGet, path, nil))
		if w.Code != http.StatusServiceUnavailable {
			t.Fatalf("expected 503 for %s without snapshot, got %d", path, w.Code)
		}
	}

	// Admin sync endpoint requires bearer token when configured.
	called := false
	admin := adminHandler(store, func(ctx context.Context) error {
		called = true
		store.current.Store(buildTestSnapshot(t, richFixture))
		return nil
	}, "secret-token")

	w := httptest.NewRecorder()
	admin.ServeHTTP(w, httptest.NewRequest(http.MethodPost, "/admin/sync", nil))
	if w.Code != http.StatusUnauthorized || called {
		t.Fatalf("expected 401 without token, got %d", w.Code)
	}

	req := httptest.NewRequest(http.MethodPost, "/admin/sync", nil)
	req.Header.Set("Authorization", "Bearer secret-token")
	w = httptest.NewRecorder()
	admin.ServeHTTP(w, req)
	if w.Code != http.StatusOK || !called {
		t.Fatalf("expected 200 with valid token, got %d %s", w.Code, w.Body.String())
	}
}
