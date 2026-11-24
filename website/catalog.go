package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"sort"
	"strings"
	"time"
)

const (
	defaultSourceURL = "https://models.dev/api.json"
	schemaVersion    = "2.0"
	sourceName       = "models.dev"
	maxCatalogBytes  = 128 << 20
	defaultMinRatio  = 0.7
)

// Object represents a raw JSON object preserving all unknown keys and exact number tokens.
type Object = map[string]json.RawMessage

// Catalog mirrors the schema 2.0 models.json document.
type Catalog struct {
	SchemaVersion   string            `json:"schemaVersion"`
	Revision        string            `json:"catalogRevision"`
	Hash            string            `json:"catalogHash"`
	GeneratedAt     string            `json:"generatedAt"`
	Source          string            `json:"source"`
	SourceURL       string            `json:"sourceURL"`
	SourceFetchedAt string            `json:"sourceFetchedAt"`
	SourceHash      string            `json:"sourceHash"`
	TotalProviders  int               `json:"totalProviders"`
	TotalModels     int               `json:"totalModels"`
	Providers       map[string]Object `json:"providers"`
}

// Manifest mirrors publication.json.
type Manifest struct {
	SchemaVersion   string `json:"schemaVersion"`
	Revision        string `json:"catalogRevision"`
	Hash            string `json:"catalogHash"`
	GeneratedAt     string `json:"generatedAt"`
	SourceFetchedAt string `json:"sourceFetchedAt"`
	SourceHash      string `json:"sourceHash"`
	Providers       int    `json:"providers"`
	Models          int    `json:"models"`
	Bytes           int    `json:"bytes"`
	SHA256          string `json:"sha256"`
	PublishedAt     string `json:"publishedAt"`
}

// NormalizeReport summarizes what normalization kept and skipped.
type NormalizeReport struct {
	SourceProviders    int
	SourceModels       int
	PublishedProviders int
	PublishedModels    int
	SkippedProviders   int
	SkippedModels      int
	Warnings           []string
}

// SyncStatus tracks synchronization health for /healthz.
type SyncStatus struct {
	LastSyncAttemptAt   string `json:"lastSyncAttemptAt,omitempty"`
	LastSyncSuccessAt   string `json:"lastSyncSuccessAt,omitempty"`
	LastSyncError       string `json:"lastSyncError,omitempty"`
	ConsecutiveFailures int    `json:"consecutiveFailures"`
}

// Snapshot holds an immutable validated catalog and its derived indices.
type Snapshot struct {
	Catalog         Catalog
	Manifest        Manifest
	Raw             []byte
	Publication     []byte
	ETag            string
	PublicationETag string
	LastModified    time.Time
	Providers       []ProviderView
	Models          []ModelView
}

func sha256Hex(b []byte) string {
	sum := sha256.Sum256(b)
	return hex.EncodeToString(sum[:])
}

func formatStamp(t time.Time) string {
	return t.UTC().Format(time.RFC3339)
}

func encodeJSON(v any) ([]byte, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		return nil, err
	}
	return bytes.TrimSuffix(buf.Bytes(), []byte("\n")), nil
}

// canonicalizeRaw recursively decodes JSON with UseNumber() and re-encodes it
// deterministically with sorted keys at every nesting level.
func canonicalizeRaw(data []byte) ([]byte, error) {
	dec := json.NewDecoder(bytes.NewReader(data))
	dec.UseNumber()
	var val any
	if err := dec.Decode(&val); err != nil {
		return nil, err
	}
	if err := ensureEOF(dec); err != nil {
		return nil, err
	}
	return encodeJSON(val)
}

func ensureEOF(dec *json.Decoder) error {
	var extra any
	if err := dec.Decode(&extra); err != io.EOF {
		if err == nil {
			return errors.New("trailing JSON data")
		}
		return err
	}
	return nil
}

func rawJSON(v any) (json.RawMessage, error) {
	b, err := encodeJSON(v)
	if err != nil {
		return nil, err
	}
	return json.RawMessage(b), nil
}

func parseObject(b json.RawMessage) Object {
	trimmed := bytes.TrimSpace(b)
	if len(trimmed) == 0 || trimmed[0] != '{' {
		return nil
	}
	var m Object
	if json.Unmarshal(trimmed, &m) != nil {
		return nil
	}
	return m
}

func parseString(b json.RawMessage) string {
	var s string
	_ = json.Unmarshal(b, &s)
	return s
}

func parseOptionalBool(b json.RawMessage) *bool {
	trimmed := string(bytes.TrimSpace(b))
	if trimmed == "true" {
		v := true
		return &v
	}
	if trimmed == "false" {
		v := false
		return &v
	}
	return nil
}

func parseOptionalInt64(b json.RawMessage) *int64 {
	if isNullOrEmpty(b) {
		return nil
	}
	var n int64
	if json.Unmarshal(b, &n) != nil {
		return nil
	}
	return &n
}

func parseOptionalFloat64(b json.RawMessage) *float64 {
	if isNullOrEmpty(b) {
		return nil
	}
	var f float64
	if json.Unmarshal(b, &f) != nil {
		return nil
	}
	return &f
}

func isNullOrEmpty(b json.RawMessage) bool {
	trimmed := bytes.TrimSpace(b)
	return len(trimmed) == 0 || bytes.Equal(trimmed, []byte("null"))
}

func isBoolToken(b json.RawMessage) bool {
	trimmed := string(bytes.TrimSpace(b))
	return trimmed == "true" || trimmed == "false"
}

var (
	modelBoolFields   = []string{"reasoning", "attachment", "tool_call", "structured_output", "open_weights", "temperature"}
	modelStringFields = []string{"id", "name", "description", "family", "canonical_model_id", "release_date", "last_updated", "knowledge", "status"}
	runtimeGuessKeys  = []string{"swiftDriver", "reasoningField", "swiftSnippet"}
)

func validModel(m Object) bool {
	for _, k := range modelBoolFields {
		if b := m[k]; !isNullOrEmpty(b) && !isBoolToken(b) {
			return false
		}
	}
	for _, k := range modelStringFields {
		if b := m[k]; !isNullOrEmpty(b) {
			var s string
			if json.Unmarshal(b, &s) != nil {
				return false
			}
		}
	}
	if b := m["limit"]; !isNullOrEmpty(b) {
		lim := parseObject(b)
		if lim == nil {
			return false
		}
		for _, k := range []string{"context", "input", "output"} {
			if v := lim[k]; !isNullOrEmpty(v) {
				var n int64
				if json.Unmarshal(v, &n) != nil || n < 0 {
					return false
				}
			}
		}
	}
	if b := m["cost"]; !isNullOrEmpty(b) && parseObject(b) == nil {
		return false
	}
	if b := m["modalities"]; !isNullOrEmpty(b) {
		mod := parseObject(b)
		if mod == nil {
			return false
		}
		for _, k := range []string{"input", "output"} {
			if v := mod[k]; !isNullOrEmpty(v) {
				var arr []json.RawMessage
				if json.Unmarshal(v, &arr) != nil {
					return false
				}
				for _, item := range arr {
					var s string
					if isNullOrEmpty(item) || json.Unmarshal(item, &s) != nil {
						return false
					}
				}
			}
		}
	}
	return true
}

func stripRuntimeGuesses(o Object) {
	for _, k := range runtimeGuessKeys {
		delete(o, k)
	}
}

func normalize(body []byte, srcURL string, now time.Time, previous *Snapshot, minRatio float64, allowShrink bool) ([]byte, NormalizeReport, error) {
	var report NormalizeReport
	if minRatio <= 0 || minRatio > 1 {
		minRatio = defaultMinRatio
	}

	var source map[string]json.RawMessage
	if err := json.Unmarshal(body, &source); err != nil || len(source) == 0 {
		return nil, report, errors.New("invalid upstream document: expected non-empty JSON object")
	}
	report.SourceProviders = len(source)

	providers := make(map[string]Object)
	objectProviders := 0
	providersWithModels := 0
	totalModels := 0

	// Sort provider IDs so warning logs and processing are deterministic.
	providerIDs := make([]string, 0, len(source))
	for pid := range source {
		providerIDs = append(providerIDs, pid)
	}
	sort.Strings(providerIDs)

	for _, pid := range providerIDs {
		rawProv := source[pid]
		p := parseObject(rawProv)
		if p == nil {
			report.SkippedProviders++
			report.Warnings = append(report.Warnings, fmt.Sprintf("provider %s: not an object", pid))
			continue
		}
		objectProviders++

		rawModels := parseObject(p["models"])
		if len(rawModels) == 0 {
			report.SkippedProviders++
			report.Warnings = append(report.Warnings, fmt.Sprintf("provider %s: missing or empty models object", pid))
			continue
		}
		providersWithModels++
		report.SourceModels += len(rawModels)

		if pid == "" {
			report.SkippedProviders++
			report.Warnings = append(report.Warnings, "empty provider id skipped")
			continue
		}

		modelIDs := make([]string, 0, len(rawModels))
		for mid := range rawModels {
			modelIDs = append(modelIDs, mid)
		}
		sort.Strings(modelIDs)

		kept := make(Object)
		for _, mid := range modelIDs {
			m := parseObject(rawModels[mid])
			if mid == "" || m == nil || !validModel(m) {
				report.SkippedModels++
				report.Warnings = append(report.Warnings, fmt.Sprintf("model %s/%s: invalid schema", pid, mid))
				continue
			}
			stripRuntimeGuesses(m)
			encodedModel, err := encodeJSON(m)
			if err != nil {
				return nil, report, err
			}
			kept[mid] = encodedModel
		}
		if len(kept) == 0 {
			report.SkippedProviders++
			report.Warnings = append(report.Warnings, fmt.Sprintf("provider %s: no valid models", pid))
			continue
		}

		stripRuntimeGuesses(p)
		modelsRaw, err := rawJSON(kept)
		if err != nil {
			return nil, report, err
		}
		countRaw, err := rawJSON(len(kept))
		if err != nil {
			return nil, report, err
		}
		var baseURLRaw json.RawMessage
		if api := parseString(p["api"]); api != "" {
			baseURLRaw, err = rawJSON(api)
		} else {
			baseURLRaw, err = rawJSON(nil)
		}
		if err != nil {
			return nil, report, err
		}

		p["models"] = modelsRaw
		p["modelCount"] = countRaw
		p["baseURL"] = baseURLRaw
		providers[pid] = p
		totalModels += len(kept)
	}

	minWithModels := objectProviders / 2
	if minWithModels < 1 {
		minWithModels = 1
	}
	if objectProviders == 0 || providersWithModels == 0 || providersWithModels < minWithModels || len(providers) == 0 || totalModels == 0 {
		return nil, report, fmt.Errorf("upstream structure drift: %d/%d providers have models", providersWithModels, objectProviders)
	}

	if previous != nil && !allowShrink {
		prev := previous.Catalog
		if prev.TotalModels > 0 && float64(totalModels) < minRatio*float64(prev.TotalModels) {
			return nil, report, fmt.Errorf("model count shrank from %d to %d (%.1f%% < %.0f%%); previous snapshot retained",
				prev.TotalModels, totalModels, 100*float64(totalModels)/float64(prev.TotalModels), minRatio*100)
		}
		if prev.TotalProviders > 0 && float64(len(providers)) < minRatio*float64(prev.TotalProviders) {
			return nil, report, fmt.Errorf("provider count shrank from %d to %d (%.1f%% < %.0f%%); previous snapshot retained",
				prev.TotalProviders, len(providers), 100*float64(len(providers))/float64(prev.TotalProviders), minRatio*100)
		}
	}

	// Build recursive canonical JSON for deterministic catalogHash & catalogRevision.
	rawCanonicalInput, err := encodeJSON(map[string]any{
		"schemaVersion": schemaVersion,
		"providers":     providers,
	})
	if err != nil {
		return nil, report, err
	}
	canonical, err := canonicalizeRaw(rawCanonicalInput)
	if err != nil {
		return nil, report, err
	}
	digest := sha256Hex(canonical)

	catalog := Catalog{
		SchemaVersion:   schemaVersion,
		Revision:        digest[:12],
		Hash:            "sha256:" + digest,
		GeneratedAt:     formatStamp(now),
		Source:          sourceName,
		SourceURL:       srcURL,
		SourceFetchedAt: formatStamp(now),
		SourceHash:      "sha256:" + sha256Hex(body),
		TotalProviders:  len(providers),
		TotalModels:     totalModels,
		Providers:       providers,
	}
	out, err := encodeJSON(catalog)
	if err != nil {
		return nil, report, err
	}
	report.PublishedProviders = len(providers)
	report.PublishedModels = totalModels
	return out, report, nil
}

func parseSnapshot(body []byte, publishedAt string) (*Snapshot, error) {
	var c Catalog
	if err := json.Unmarshal(body, &c); err != nil {
		return nil, err
	}
	if c.SchemaVersion != schemaVersion || c.Revision == "" || c.Hash == "" || len(c.Providers) == 0 {
		return nil, errors.New("invalid catalog header")
	}

	counted := 0
	for pid, p := range c.Providers {
		if pid == "" {
			return nil, errors.New("empty provider id")
		}
		for _, k := range runtimeGuessKeys {
			if _, ok := p[k]; ok {
				return nil, fmt.Errorf("unexpected runtime guess field %q on provider %s", k, pid)
			}
		}
		models := parseObject(p["models"])
		if len(models) == 0 {
			return nil, fmt.Errorf("provider %s has no models", pid)
		}
		for mid, r := range models {
			m := parseObject(r)
			if mid == "" || m == nil || !validModel(m) {
				return nil, fmt.Errorf("invalid model %s/%s", pid, mid)
			}
			for _, k := range runtimeGuessKeys {
				if _, ok := m[k]; ok {
					return nil, fmt.Errorf("unexpected runtime guess field %q on model %s/%s", k, pid, mid)
				}
			}
			counted++
		}
	}
	if c.TotalModels != counted || c.TotalProviders != len(c.Providers) {
		return nil, fmt.Errorf("catalog count mismatch: header(%d/%d) != actual(%d/%d)",
			c.TotalProviders, c.TotalModels, len(c.Providers), counted)
	}

	if publishedAt == "" {
		publishedAt = c.GeneratedAt
	}
	rawDigest := sha256Hex(body)
	manifest := Manifest{
		SchemaVersion:   c.SchemaVersion,
		Revision:        c.Revision,
		Hash:            c.Hash,
		GeneratedAt:     c.GeneratedAt,
		SourceFetchedAt: c.SourceFetchedAt,
		SourceHash:      c.SourceHash,
		Providers:       c.TotalProviders,
		Models:          c.TotalModels,
		Bytes:           len(body),
		SHA256:          rawDigest,
		PublishedAt:     publishedAt,
	}
	pubBytes, err := encodeJSON(manifest)
	if err != nil {
		return nil, err
	}
	lastMod, err := time.Parse(time.RFC3339, publishedAt)
	if err != nil {
		lastMod, _ = time.Parse(time.RFC3339, c.GeneratedAt)
	}

	snap := &Snapshot{
		Catalog:         c,
		Manifest:        manifest,
		Raw:             body,
		Publication:     pubBytes,
		ETag:            `"` + rawDigest + `"`,
		PublicationETag: `"` + sha256Hex(pubBytes) + `"`,
		LastModified:    lastMod.UTC(),
	}
	snap.buildIndex()
	return snap, nil
}

type providerStatusEntry struct {
	Name      string   `json:"name"`
	Endpoints []string `json:"endpoints"`
	Status    string   `json:"status"`
	Protocols []string `json:"protocols"`
}

var embeddedProviderStatuses = loadEmbeddedProviderStatuses()

func loadEmbeddedProviderStatuses() map[string]providerStatusEntry {
	out := make(map[string]providerStatusEntry)
	raw, err := assets.ReadFile("static/provider-status.json")
	if err != nil {
		return out
	}
	_ = json.Unmarshal(raw, &out)
	return out
}

func formatProviderStatusLabel(raw string) string {
	switch raw {
	case "full_support":
		return "Full Support"
	case "live_verified":
		return "Live Verified"
	case "mock_tested":
		return "Mock Tested"
	case "compatible":
		return "Compatible"
	default:
		return "Discovered"
	}
}

func (s *Snapshot) buildIndex() {
	s.Providers = make([]ProviderView, 0, len(s.Catalog.Providers))
	for pid, p := range s.Catalog.Providers {
		models := parseObject(p["models"])
		var env []string
		_ = json.Unmarshal(p["env"], &env)
		statusLabel := "Discovered"
		var protocols string
		if st, ok := embeddedProviderStatuses[pid]; ok {
			statusLabel = formatProviderStatusLabel(st.Status)
			protocols = strings.Join(st.Protocols, ", ")
		}
		s.Providers = append(s.Providers, ProviderView{
			ID:        pid,
			Name:      fallbackString(parseString(p["name"]), pid),
			Endpoint:  fallbackString(parseString(p["baseURL"]), parseString(p["api"])),
			Doc:       parseString(p["doc"]),
			Env:       strings.Join(env, ", "),
			Count:     len(models),
			Status:    statusLabel,
			Protocols: protocols,
		})
	}
	sortProviders(s.Providers)

	s.Models = make([]ModelView, 0, s.Catalog.TotalModels)
	for _, prov := range s.Providers {
		rawModels := parseObject(s.Catalog.Providers[prov.ID]["models"])
		group := make([]ModelView, 0, len(rawModels))
		for mid, b := range rawModels {
			m := parseObject(b)
			lim := parseObject(m["limit"])
			cost := parseObject(m["cost"])
			modal := parseObject(m["modalities"])

			var inputs []string
			_ = json.Unmarshal(modal["input"], &inputs)
			vision := false
			for _, v := range inputs {
				if strings.HasPrefix(strings.ToLower(v), "image") {
					vision = true
					break
				}
			}

			ctxPtr := parseOptionalInt64(lim["context"])
			outPtr := parseOptionalInt64(lim["output"])
			inPricePtr := parseOptionalFloat64(cost["input"])
			outPricePtr := parseOptionalFloat64(cost["output"])

			group = append(group, ModelView{
				ProviderID:      prov.ID,
				ProviderName:    prov.Name,
				ID:              mid,
				Name:            fallbackString(parseString(m["name"]), mid),
				Family:          parseString(m["family"]),
				Description:     parseString(m["description"]),
				Release:         parseString(m["release_date"]),
				Updated:         parseString(m["last_updated"]),
				Status:          parseString(m["status"]),
				Context:         formatTokenLimit(ctxPtr),
				Output:          formatTokenLimit(outPtr),
				InputPrice:      formatPriceUSD(inPricePtr),
				OutputPrice:     formatPriceUSD(outPricePtr),
				PriceKnown:      inPricePtr != nil || outPricePtr != nil,
				ContextN:        ctxPtr,
				OutputN:         outPtr,
				InputPriceN:     inPricePtr,
				OutputPriceN:    outPricePtr,
				Reasoning:       parseOptionalBool(m["reasoning"]),
				Vision:          vision,
				Attachments:     parseOptionalBool(m["attachment"]),
				Tools:           parseOptionalBool(m["tool_call"]),
				Structured:      parseOptionalBool(m["structured_output"]),
				OpenWeights:     parseOptionalBool(m["open_weights"]),
				SupportsTemp:    parseOptionalBool(m["temperature"]),
				InputModalities: inputs,
			})
		}
		sortModels(group)
		s.Models = append(s.Models, group...)
	}
}
