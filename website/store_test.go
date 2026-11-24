package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

func TestStoreSyncRetryShrinkAndRestartRecovery(t *testing.T) {
	var body atomic.Value
	body.Store(richFixture)
	var failCount atomic.Int32

	up := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if failCount.Load() > 0 {
			failCount.Add(-1)
			http.Error(w, "upstream error", http.StatusBadGateway)
			return
		}
		_, _ = w.Write([]byte(body.Load().(string)))
	}))
	defer up.Close()

	dir := t.TempDir()
	store := newStore(dir, defaultMinRatio)
	store.retryBaseDelay = time.Millisecond

	// Retry recovers from transient 502s.
	failCount.Store(2)
	if err := store.sync(context.Background(), up.Client(), up.URL, false); err != nil {
		t.Fatalf("expected retry to succeed: %v", err)
	}
	first := store.snapshot()
	if first == nil || first.Catalog.TotalModels != 3 {
		t.Fatalf("unexpected initial snapshot: %+v", first)
	}

	// Invalid upstream leaves last-known-good intact on disk and in memory.
	body.Store(`{}`)
	if err := store.sync(context.Background(), up.Client(), up.URL, false); err == nil {
		t.Fatal("expected empty upstream to fail")
	}
	if store.snapshot() != first {
		t.Fatal("in-memory snapshot changed after failed sync")
	}
	if store.syncStatus().ConsecutiveFailures != 1 {
		t.Fatalf("expected consecutiveFailures=1, got %d", store.syncStatus().ConsecutiveFailures)
	}

	// >30% shrink rejected unless allowShrink=true.
	body.Store(`{"p1":{"name":"Provider 2","models":{"m1":{"name":"Only One"}}}}`)
	if err := store.sync(context.Background(), up.Client(), up.URL, false); err == nil {
		t.Fatal("expected >30% shrink to be rejected")
	}
	if err := store.sync(context.Background(), up.Client(), up.URL, true); err != nil {
		t.Fatalf("expected allowShrink=true to succeed: %v", err)
	}

	// publication.json and publication.json.prev both persisted.
	if _, err := os.Stat(filepath.Join(dir, "publication.json")); err != nil {
		t.Fatalf("missing publication.json: %v", err)
	}
	if _, err := os.Stat(filepath.Join(dir, "publication.json.prev")); err != nil {
		t.Fatalf("missing publication.json.prev: %v", err)
	}

	// Restart recovery preserves publishedAt and validates via CheckSnapshot.
	manifest, err := CheckSnapshot(dir)
	if err != nil || manifest.Models != 1 {
		t.Fatalf("CheckSnapshot failed: %v %+v", err, manifest)
	}
	restarted := newStore(dir, defaultMinRatio)
	if err := restarted.load(); err != nil {
		t.Fatalf("restart load failed: %v", err)
	}
	if restarted.snapshot().Manifest.PublishedAt != store.snapshot().Manifest.PublishedAt {
		t.Fatalf("publishedAt lost across restart")
	}

	// Corrupt models.json on disk is rejected by load().
	if err := os.WriteFile(filepath.Join(dir, "models.json"), []byte("{corrupt"), 0o644); err != nil {
		t.Fatal(err)
	}
	corruptStore := newStore(dir, defaultMinRatio)
	if err := corruptStore.load(); err == nil {
		t.Fatal("expected corrupt snapshot to be rejected")
	}
}

func TestStoreConcurrentReadersAndWriters(t *testing.T) {
	var seq atomic.Int64
	up := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		n := seq.Add(1)
		payload := fmt.Sprintf(`{"p":{"name":"P","models":{"m1":{"name":"M1-%d"},"m2":{"name":"M2"}}}}`, n)
		_, _ = w.Write([]byte(payload))
	}))
	defer up.Close()

	store := newStore(t.TempDir(), defaultMinRatio)
	store.retryBaseDelay = time.Millisecond
	if err := store.sync(context.Background(), up.Client(), up.URL, false); err != nil {
		t.Fatal(err)
	}
	h := publicHandler(store)

	var wg sync.WaitGroup
	for i := 0; i < 6; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := 0; j < 15; j++ {
				w := httptest.NewRecorder()
				h.ServeHTTP(w, httptest.NewRequest(http.MethodGet, "/models.json", nil))
				if w.Code != http.StatusOK || !bytes.Contains(w.Body.Bytes(), []byte(`"schemaVersion":"2.0"`)) {
					t.Errorf("unexpected response during concurrent read: %d", w.Code)
					return
				}
				var cat Catalog
				if err := json.Unmarshal(w.Body.Bytes(), &cat); err != nil {
					t.Errorf("invalid JSON during concurrent read: %v", err)
					return
				}
			}
		}()
	}
	for i := 0; i < 3; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := 0; j < 4; j++ {
				if err := store.sync(context.Background(), up.Client(), up.URL, false); err != nil {
					t.Errorf("concurrent sync failed: %v", err)
					return
				}
			}
		}()
	}
	wg.Wait()
}
