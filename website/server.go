package main

import (
	"bytes"
	"context"
	"crypto/subtle"
	"embed"
	"encoding/json"
	"io/fs"
	"log"
	"net/http"
	"net/url"
	"strings"
	"time"
)

//go:embed templates/partials/*.html templates/pages/*.html static/*
var assets embed.FS

var templateFuncs = templateFuncsMap()

func templateFuncsMap() map[string]any {
	return map[string]any{
		"safeURL": func(s string) string {
			u, err := url.Parse(s)
			if err != nil || u.Host == "" || (u.Scheme != "https" && u.Scheme != "http") {
				return ""
			}
			return s
		},
		"shortHash": func(s string) string {
			s = strings.TrimPrefix(s, "sha256:")
			if len(s) > 12 {
				return s[:12]
			}
			return s
		},
	}
}

type pageTemplates struct {
	catalog   *templateSet
	sdk       *templateSet
	protocols *templateSet
	providers *templateSet
	gateway   *templateSet
}

// Each page is parsed alongside partials so page-specific blocks never collide.
func parsePageTemplates() pageTemplates {
	return pageTemplates{
		catalog:   mustParsePage("templates/pages/catalog.html"),
		sdk:       mustParsePage("templates/pages/sdk.html"),
		protocols: mustParsePage("templates/pages/protocols.html"),
		providers: mustParsePage("templates/pages/providers.html"),
		gateway:   mustParsePage("templates/pages/gateway.html"),
	}
}

func writeJSONResponse(w http.ResponseWriter, r *http.Request, body []byte, etag string, lastMod time.Time, cacheControl string) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.Header().Set("ETag", etag)
	w.Header().Set("Cache-Control", cacheControl)
	w.Header().Set("Access-Control-Allow-Origin", "*")
	w.Header().Set("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS")
	w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization, If-None-Match, If-Modified-Since")
	w.Header().Set("Access-Control-Expose-Headers", "ETag")
	if r.Method == http.MethodOptions {
		w.WriteHeader(http.StatusNoContent)
		return
	}
	http.ServeContent(w, r, "data.json", lastMod, bytes.NewReader(body))
}

func renderHTML(w http.ResponseWriter, r *http.Request, tmpl *templateSet, name string, data any, status int) {
	var buf bytes.Buffer
	if err := tmpl.ExecuteTemplate(&buf, name, data); err != nil {
		log.Printf("template %s: %v", name, err)
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	if status >= 500 {
		w.Header().Set("Cache-Control", "no-store")
		w.WriteHeader(status)
	} else {
		w.Header().Set("Cache-Control", "public, max-age=60, must-revalidate")
	}
	if r.Method != http.MethodHead {
		_, _ = w.Write(buf.Bytes())
	}
}

func publicHandler(s *Store) http.Handler {
	pages := parsePageTemplates()
	mux := http.NewServeMux()

	staticFS, err := fs.Sub(assets, "static")
	if err != nil {
		panic(err)
	}
	staticServer := http.FileServer(http.FS(staticFS))
	mux.Handle("GET /static/", http.StripPrefix("/static/", http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "public, max-age=3600")
		staticServer.ServeHTTP(w, r)
	})))

	// Preserve legacy /beian-icon.png URL in addition to /static/beian-icon.png.
	mux.HandleFunc("GET /beian-icon.png", func(w http.ResponseWriter, r *http.Request) {
		data, err := assets.ReadFile("static/beian-icon.png")
		if err != nil {
			http.NotFound(w, r)
			return
		}
		w.Header().Set("Content-Type", "image/png")
		w.Header().Set("Cache-Control", "public, max-age=86400")
		http.ServeContent(w, r, "beian-icon.png", time.Time{}, bytes.NewReader(data))
	})

	mux.HandleFunc("GET /index.html", func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, "/", http.StatusMovedPermanently)
	})

	for _, path := range []string{"/models.json", "/publication.json"} {
		endpoint := path
		h := func(w http.ResponseWriter, r *http.Request) {
			snap := s.snapshot()
			if snap == nil {
				w.Header().Set("Cache-Control", "no-store")
				http.Error(w, "catalog unavailable", http.StatusServiceUnavailable)
				return
			}
			if endpoint == "/models.json" {
				writeJSONResponse(w, r, snap.Raw, snap.ETag, snap.LastModified, "public, max-age=1800, stale-while-revalidate=86400")
				return
			}
			writeJSONResponse(w, r, snap.Publication, snap.PublicationETag, snap.LastModified, "public, max-age=60, must-revalidate")
		}
		mux.HandleFunc("GET "+endpoint, h)
		mux.HandleFunc("OPTIONS "+endpoint, h)
	}

	mux.HandleFunc("GET /healthz", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json; charset=utf-8")
		w.Header().Set("Cache-Control", "no-store")

		snap := s.snapshot()
		syncSt := s.syncStatus()
		payload := map[string]any{
			"status":              "ok",
			"lastSyncAttemptAt":   syncSt.LastSyncAttemptAt,
			"lastSyncSuccessAt":   syncSt.LastSyncSuccessAt,
			"lastSyncError":       syncSt.LastSyncError,
			"consecutiveFailures": syncSt.ConsecutiveFailures,
		}
		if snap == nil {
			payload["status"] = "unavailable"
			w.WriteHeader(http.StatusServiceUnavailable)
			_ = json.NewEncoder(w).Encode(payload)
			return
		}
		payload["catalogRevision"] = snap.Catalog.Revision
		payload["providers"] = snap.Catalog.TotalProviders
		payload["models"] = snap.Catalog.TotalModels
		payload["generatedAt"] = snap.Catalog.GeneratedAt
		payload["publishedAt"] = snap.Manifest.PublishedAt
		_ = json.NewEncoder(w).Encode(payload)
	})

	mux.HandleFunc("GET /{$}", func(w http.ResponseWriter, r *http.Request) {
		v := buildCatalogView(s.snapshot(), r.URL.Query())
		status := http.StatusOK
		if v.Unavailable {
			status = http.StatusServiceUnavailable
		}
		renderHTML(w, r, pages.catalog, "catalog.html", v, status)
	})

	mux.HandleFunc("GET /sdk", func(w http.ResponseWriter, r *http.Request) {
		v := buildStaticView(s.snapshot(), "sdk", "SDK 文档与多语言支持 — LingXi Models")
		renderHTML(w, r, pages.sdk, "sdk.html", v, http.StatusOK)
	})

	mux.HandleFunc("GET /protocols", func(w http.ResponseWriter, r *http.Request) {
		v := buildStaticView(s.snapshot(), "protocols", "协议支持状态 — LingXi Models")
		renderHTML(w, r, pages.protocols, "protocols.html", v, http.StatusOK)
	})

	mux.HandleFunc("GET /providers", func(w http.ResponseWriter, r *http.Request) {
		v := buildProvidersView(s.snapshot(), r.URL.Query())
		status := http.StatusOK
		if v.Unavailable {
			status = http.StatusServiceUnavailable
		}
		renderHTML(w, r, pages.providers, "providers.html", v, status)
	})

	mux.HandleFunc("GET /gateway", func(w http.ResponseWriter, r *http.Request) {
		v := buildStaticView(s.snapshot(), "gateway", "HTTP Gateway 部署与安全指南 — LingXi Models")
		renderHTML(w, r, pages.gateway, "gateway.html", v, http.StatusOK)
	})

	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("X-Content-Type-Options", "nosniff")
		w.Header().Set("Referrer-Policy", "strict-origin-when-cross-origin")
		w.Header().Set("X-Frame-Options", "DENY")
		w.Header().Set("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'")
		mux.ServeHTTP(w, r)
	})
}

func adminHandler(s *Store, syncFn func(context.Context) error, token string) http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("POST /admin/sync", func(w http.ResponseWriter, r *http.Request) {
		if token != "" {
			auth := r.Header.Get("Authorization")
			expected := "Bearer " + token
			if subtle.ConstantTimeCompare([]byte(auth), []byte(expected)) != 1 {
				http.Error(w, "unauthorized", http.StatusUnauthorized)
				return
			}
		}
		ctx, cancel := context.WithTimeout(r.Context(), 90*time.Second)
		defer cancel()
		if err := syncFn(ctx); err != nil {
			w.Header().Set("Content-Type", "application/json; charset=utf-8")
			w.WriteHeader(http.StatusBadGateway)
			_ = json.NewEncoder(w).Encode(map[string]any{"ok": false, "error": err.Error()})
			return
		}
		snap := s.snapshot()
		w.Header().Set("Content-Type", "application/json; charset=utf-8")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"ok":              true,
			"catalogRevision": snap.Catalog.Revision,
			"providers":       snap.Catalog.TotalProviders,
			"models":          snap.Catalog.TotalModels,
		})
	})
	return mux
}
