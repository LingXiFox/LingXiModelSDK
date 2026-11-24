package catalog_test

import (
	"bytes"
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	catalog "github.com/LingXiFox/LingXiModelSDK/sdk/go"
)

func TestLoaderETagOfflineAndDiskRoundTrip(t *testing.T) {
	fixture := readFixture(t, "v2_catalog.json")
	var mode atomic.Int32
	var hits atomic.Int32

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		switch mode.Load() {
		case 0:
			if r.Header.Get("If-None-Match") == `"rev-1"` {
				w.WriteHeader(http.StatusNotModified)
				return
			}
			w.Header().Set("ETag", `"rev-1"`)
			_, _ = w.Write(fixture)
		case 1:
			http.Error(w, "down", http.StatusServiceUnavailable)
		}
	}))
	defer srv.Close()

	dir := t.TempDir()
	cfg := catalog.Config{
		Endpoint:       srv.URL,
		MaxAge:         time.Hour,
		CacheDirectory: dir,
	}
	loader := catalog.NewLoader(cfg, srv.Client())

	cat1, err := loader.Load(context.Background(), false)
	if err != nil || cat1.Revision.CatalogRevision != "conf-rev-001" {
		t.Fatalf("initial load failed: %v", err)
	}
	// Fresh cache skips network.
	if _, err := loader.Load(context.Background(), false); err != nil {
		t.Fatal(err)
	}
	if hits.Load() != 1 {
		t.Fatalf("expected fresh cache to skip network, got %d hits", hits.Load())
	}

	// forceRefresh sends If-None-Match and handles 304.
	if _, err := loader.Load(context.Background(), true); err != nil {
		t.Fatal(err)
	}
	if hits.Load() != 2 {
		t.Fatalf("expected 2 hits after forceRefresh, got %d", hits.Load())
	}

	// New cold loader falls back to disk cache when upstream returns 503.
	mode.Store(1)
	cfg.MaxAge = 0
	cold := catalog.NewLoader(cfg, srv.Client())
	catFallback, err := cold.Load(context.Background(), false)
	if err != nil || catFallback.Revision.CatalogRevision != "conf-rev-001" {
		t.Fatalf("fallback load failed: %v", err)
	}

	// Raw document on disk preserves exact bytes.
	stored, err := os.ReadFile(filepath.Join(dir, "models.json"))
	if err != nil || !bytes.Equal(stored, fixture) {
		t.Fatalf("disk cache bytes mismatch")
	}

	// No cache + 503 returns ErrUnexpectedStatus.
	emptyLoader := catalog.NewLoader(catalog.Config{Endpoint: srv.URL, MaxAge: 0}, srv.Client())
	_, err = emptyLoader.Load(context.Background(), false)
	var ce *catalog.CatalogError
	if !errors.As(err, &ce) || ce.Kind != catalog.ErrUnexpectedStatus || ce.StatusCode != 503 {
		t.Fatalf("expected 503 error, got %v", err)
	}
}
