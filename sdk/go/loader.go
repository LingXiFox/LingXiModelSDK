package catalog

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"time"
)

// DefaultEndpoint is the public LingXi model catalog URL.
const DefaultEndpoint = "https://models.lingxifox.cn/models.json"

// Config configures catalog fetching and caching.
type Config struct {
	Endpoint       string
	MaxAge         time.Duration
	CacheDirectory string
	RequestTimeout time.Duration
	UserAgent      string
}

// DefaultConfig returns standard configuration pointing at models.lingxifox.cn.
func DefaultConfig() Config {
	return Config{
		Endpoint:       DefaultEndpoint,
		MaxAge:         30 * time.Minute,
		RequestTimeout: 30 * time.Second,
		UserAgent:      "LingXiModelSDK-Go/2.0",
	}
}

// Transport abstracts HTTP requests for testability.
type Transport interface {
	Do(req *http.Request) (*http.Response, error)
}

type cacheMetadata struct {
	ETag      string    `json:"etag,omitempty"`
	FetchedAt time.Time `json:"fetchedAt"`
}

// CachedCatalog holds a decoded catalog and revalidation metadata.
type CachedCatalog struct {
	Catalog   *Catalog
	FetchedAt time.Time
	ETag      string
}

// IsFresh reports whether the cached catalog is still within maxAge.
func (c *CachedCatalog) IsFresh(maxAge time.Duration, now time.Time) bool {
	return maxAge > 0 && now.Sub(c.FetchedAt) < maxAge
}

// Loader fetches, caches, and revalidates the LingXi model catalog.
type Loader struct {
	config    Config
	transport Transport
	now       func() time.Time

	mu     sync.Mutex
	memory *CachedCatalog
}

// NewLoader creates a Loader with optional custom transport.
func NewLoader(cfg Config, transport Transport) *Loader {
	if cfg.Endpoint == "" {
		cfg.Endpoint = DefaultEndpoint
	}
	if cfg.RequestTimeout <= 0 {
		cfg.RequestTimeout = 30 * time.Second
	}
	if cfg.UserAgent == "" {
		cfg.UserAgent = "LingXiModelSDK-Go/2.0"
	}
	if transport == nil {
		transport = &http.Client{Timeout: cfg.RequestTimeout}
	}
	return &Loader{
		config:    cfg,
		transport: transport,
		now:       time.Now,
	}
}

// Cached returns the newest valid copy in memory or on disk without network I/O.
func (l *Loader) Cached() *CachedCatalog {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.cachedLocked()
}

func (l *Loader) cachedLocked() *CachedCatalog {
	if l.memory != nil {
		return l.memory
	}
	if l.config.CacheDirectory == "" {
		return nil
	}
	data, err := os.ReadFile(filepath.Join(l.config.CacheDirectory, "models.json"))
	if err != nil {
		return nil
	}
	cat, err := Decode(data)
	if err != nil {
		return nil
	}
	var meta cacheMetadata
	if metaBytes, err := os.ReadFile(filepath.Join(l.config.CacheDirectory, "models.json.meta.json")); err == nil {
		_ = json.Unmarshal(metaBytes, &meta)
	}
	fetchedAt := meta.FetchedAt
	if fetchedAt.IsZero() && cat.Revision.GeneratedAt != nil {
		fetchedAt = *cat.Revision.GeneratedAt
	}
	l.memory = &CachedCatalog{
		Catalog:   cat,
		FetchedAt: fetchedAt,
		ETag:      meta.ETag,
	}
	return l.memory
}

// Load fetches or revalidates the catalog, falling back to any cached copy on failure.
func (l *Loader) Load(ctx context.Context, forceRefresh bool) (*Catalog, error) {
	l.mu.Lock()
	defer l.mu.Unlock()

	cached := l.cachedLocked()
	if !forceRefresh && cached != nil && cached.IsFresh(l.config.MaxAge, l.now()) {
		return cached.Catalog, nil
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, l.config.Endpoint, nil)
	if err != nil {
		if cached != nil {
			return cached.Catalog, nil
		}
		return nil, &CatalogError{Kind: ErrTransport, Message: err.Error(), Cause: err}
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", l.config.UserAgent)
	if cached != nil && cached.ETag != "" {
		req.Header.Set("If-None-Match", cached.ETag)
	}

	resp, err := l.transport.Do(req)
	if err != nil {
		if cached != nil {
			return cached.Catalog, nil
		}
		return nil, &CatalogError{Kind: ErrTransport, Message: err.Error(), Cause: err}
	}
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusNotModified && cached != nil {
		now := l.now().UTC()
		etag := resp.Header.Get("ETag")
		if etag == "" {
			etag = cached.ETag
		}
		l.memory = &CachedCatalog{
			Catalog:   cached.Catalog,
			FetchedAt: now,
			ETag:      etag,
		}
		l.persistMeta(etag, now)
		return cached.Catalog, nil
	}

	if resp.StatusCode < 200 || resp.StatusCode > 299 {
		if cached != nil {
			return cached.Catalog, nil
		}
		return nil, &CatalogError{Kind: ErrUnexpectedStatus, StatusCode: resp.StatusCode}
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		if cached != nil {
			return cached.Catalog, nil
		}
		return nil, &CatalogError{Kind: ErrTransport, Message: err.Error(), Cause: err}
	}

	cat, err := Decode(body)
	if err != nil {
		if cached != nil {
			return cached.Catalog, nil
		}
		return nil, err
	}

	now := l.now().UTC()
	etag := resp.Header.Get("ETag")
	l.memory = &CachedCatalog{
		Catalog:   cat,
		FetchedAt: now,
		ETag:      etag,
	}
	l.persistCatalog(body, etag, now)
	return cat, nil
}

func (l *Loader) persistCatalog(raw []byte, etag string, fetchedAt time.Time) {
	if l.config.CacheDirectory == "" {
		return
	}
	_ = writeAtomicFile(filepath.Join(l.config.CacheDirectory, "models.json"), raw)
	l.persistMeta(etag, fetchedAt)
}

func (l *Loader) persistMeta(etag string, fetchedAt time.Time) {
	if l.config.CacheDirectory == "" {
		return
	}
	if b, err := json.Marshal(cacheMetadata{ETag: etag, FetchedAt: fetchedAt}); err == nil {
		_ = writeAtomicFile(filepath.Join(l.config.CacheDirectory, "models.json.meta.json"), b)
	}
}

func writeAtomicFile(path string, data []byte) error {
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	tmp, err := os.CreateTemp(dir, ".cache-*")
	if err != nil {
		return err
	}
	tmpPath := tmp.Name()
	if _, err := tmp.Write(data); err != nil {
		_ = tmp.Close()
		_ = os.Remove(tmpPath)
		return err
	}
	if err := tmp.Close(); err != nil {
		_ = os.Remove(tmpPath)
		return err
	}
	return os.Rename(tmpPath, path)
}
