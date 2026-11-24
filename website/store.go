package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"time"
)

const (
	catalogFilename     = "models.json"
	publicationFilename = "publication.json"
)

// Store manages the persistent catalog snapshot on disk and in memory.
type Store struct {
	dir            string
	minModelRatio  float64
	retryBaseDelay time.Duration

	current  atomic.Pointer[Snapshot]
	mu       sync.Mutex
	statusMu sync.RWMutex
	status   SyncStatus
}

func newStore(dir string, minRatio float64) *Store {
	if minRatio <= 0 || minRatio > 1 {
		minRatio = defaultMinRatio
	}
	return &Store{
		dir:            dir,
		minModelRatio:  minRatio,
		retryBaseDelay: 2 * time.Second,
	}
}

func (s *Store) load() error {
	body, err := os.ReadFile(filepath.Join(s.dir, catalogFilename))
	if err != nil {
		return err
	}
	snap, err := parseSnapshot(body, "")
	if err != nil {
		return err
	}
	if pubBytes, err := os.ReadFile(filepath.Join(s.dir, publicationFilename)); err == nil {
		var m Manifest
		if json.Unmarshal(pubBytes, &m) == nil &&
			m.SHA256 == sha256Hex(body) &&
			m.Revision == snap.Catalog.Revision &&
			m.PublishedAt != "" {
			if preserved, err := parseSnapshot(body, m.PublishedAt); err == nil {
				snap = preserved
			}
		}
	}
	s.current.Store(snap)
	return nil
}

func (s *Store) snapshot() *Snapshot {
	return s.current.Load()
}

func (s *Store) syncStatus() SyncStatus {
	s.statusMu.RLock()
	defer s.statusMu.RUnlock()
	return s.status
}

func (s *Store) recordAttempt(at time.Time) {
	s.statusMu.Lock()
	defer s.statusMu.Unlock()
	s.status.LastSyncAttemptAt = formatStamp(at)
}

func (s *Store) recordResult(at time.Time, err error) {
	s.statusMu.Lock()
	defer s.statusMu.Unlock()
	if err != nil {
		s.status.LastSyncError = err.Error()
		s.status.ConsecutiveFailures++
		return
	}
	s.status.LastSyncSuccessAt = formatStamp(at)
	s.status.LastSyncError = ""
	s.status.ConsecutiveFailures = 0
}

type httpStatusError struct {
	code int
}

func (e httpStatusError) Error() string {
	return fmt.Sprintf("upstream HTTP %d", e.code)
}

func (e httpStatusError) retryable() bool {
	return e.code == http.StatusTooManyRequests || e.code >= 500
}

func fetchUpstream(ctx context.Context, client *http.Client, srcURL string) ([]byte, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, srcURL, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "LingXiModelSync/2.0 (https://models.lingxifox.cn)")

	resp, err := client.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, httpStatusError{code: resp.StatusCode}
	}
	b, err := io.ReadAll(io.LimitReader(resp.Body, maxCatalogBytes+1))
	if err != nil {
		return nil, err
	}
	if len(b) > maxCatalogBytes {
		return nil, errors.New("upstream document exceeds maximum size")
	}
	return b, nil
}

// writeAtomicVerified writes data to a temporary file, syncs, closes, reopens to
// verify the SHA-256 digest and optional schema callback, and atomically renames.
func writeAtomicVerified(path string, data []byte, verify func([]byte) error) error {
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	tmp, err := os.CreateTemp(dir, ".catalog-*")
	if err != nil {
		return err
	}
	tmpPath := tmp.Name()
	cleanup := true
	defer func() {
		if cleanup {
			_ = os.Remove(tmpPath)
		}
	}()

	if err := tmp.Chmod(0o644); err != nil {
		_ = tmp.Close()
		return err
	}
	if _, err := tmp.Write(data); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Sync(); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}

	reread, err := os.ReadFile(tmpPath)
	if err != nil {
		return err
	}
	if sha256Hex(reread) != sha256Hex(data) {
		return errors.New("temporary file verification failed: checksum mismatch")
	}
	if verify != nil {
		if err := verify(reread); err != nil {
			return fmt.Errorf("temporary file verification failed: %w", err)
		}
	}
	if err := os.Rename(tmpPath, path); err != nil {
		return err
	}
	cleanup = false
	return nil
}

func (s *Store) sync(ctx context.Context, client *http.Client, srcURL string, allowShrink bool) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	s.recordAttempt(time.Now())

	var body []byte
	var err error
	delay := s.retryBaseDelay
	if delay <= 0 {
		delay = 2 * time.Second
	}

	for attempt := 0; attempt < 3; attempt++ {
		if attempt > 0 {
			wait := time.Duration(attempt) * delay
			select {
			case <-ctx.Done():
				s.recordResult(time.Now(), ctx.Err())
				return ctx.Err()
			case <-time.After(wait):
			}
		}
		body, err = fetchUpstream(ctx, client, srcURL)
		if err == nil {
			break
		}
		var statusErr httpStatusError
		if errors.As(err, &statusErr) && !statusErr.retryable() {
			break
		}
	}
	if err != nil {
		s.recordResult(time.Now(), err)
		return err
	}

	now := time.Now().UTC()
	normalized, _, err := normalize(body, srcURL, now, s.current.Load(), s.minModelRatio, allowShrink)
	if err != nil {
		s.recordResult(time.Now(), err)
		return err
	}
	snap, err := parseSnapshot(normalized, formatStamp(now))
	if err != nil {
		s.recordResult(time.Now(), err)
		return err
	}

	catalogPath := filepath.Join(s.dir, catalogFilename)
	if err := writeAtomicVerified(catalogPath, normalized, func(b []byte) error {
		_, e := parseSnapshot(b, formatStamp(now))
		return e
	}); err != nil {
		s.recordResult(time.Now(), err)
		return err
	}

	// Persist publication.json alongside models.json and keep .prev for audit parity.
	pubPath := filepath.Join(s.dir, publicationFilename)
	if prevPub, readErr := os.ReadFile(pubPath); readErr == nil {
		_ = writeAtomicVerified(pubPath+".prev", prevPub, nil)
	}
	prettyPub, err := json.MarshalIndent(snap.Manifest, "", "  ")
	if err == nil {
		prettyPub = append(prettyPub, '\n')
		_ = writeAtomicVerified(pubPath, prettyPub, nil)
	}

	s.current.Store(snap)
	s.recordResult(now, nil)
	return nil
}

// CheckSnapshot validates an existing models.json on disk without network access.
func CheckSnapshot(dir string) (*Manifest, error) {
	body, err := os.ReadFile(filepath.Join(dir, catalogFilename))
	if err != nil {
		return nil, err
	}
	snap, err := parseSnapshot(body, "")
	if err != nil {
		return nil, err
	}
	m := snap.Manifest
	return &m, nil
}
