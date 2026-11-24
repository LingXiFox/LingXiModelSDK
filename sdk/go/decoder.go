package catalog

import (
	"bytes"
	"encoding/json"
	"strings"
	"time"
)

type rawObject = map[string]json.RawMessage

// Decode parses a published models.json document into a Catalog without network I/O.
func Decode(data []byte) (*Catalog, error) {
	root := parseObject(data)
	if root == nil {
		return nil, &CatalogError{Kind: ErrInvalidCatalog, Message: "document root is not a JSON object"}
	}

	providersObj := parseObject(root["providers"])
	if len(providersObj) == 0 {
		return nil, &CatalogError{Kind: ErrInvalidCatalog, Message: "providers missing or empty"}
	}

	schemaVersion := "1.0"
	if sv := parseString(root["schemaVersion"]); sv != nil {
		schemaVersion = *sv
	} else if v := parseString(root["version"]); v != nil {
		schemaVersion = *v
	}
	major := strings.SplitN(schemaVersion, ".", 2)[0]
	if major != "1" && major != "2" {
		return nil, &CatalogError{Kind: ErrUnsupportedSchemaVersion, Version: schemaVersion}
	}

	var generatedAt *time.Time
	if s := parseString(root["generatedAt"]); s != nil {
		generatedAt = parseTimestamp(*s)
	} else if s := parseString(root["updatedAt"]); s != nil {
		generatedAt = parseTimestamp(*s)
	}

	providers := make([]Provider, 0, len(providersObj))
	for pid, rawProv := range providersObj {
		fields := parseObject(rawProv)
		if fields == nil {
			continue
		}
		if p := decodeProvider(pid, fields); p != nil {
			providers = append(providers, *p)
		}
	}
	if len(providers) == 0 {
		return nil, &CatalogError{Kind: ErrInvalidCatalog, Message: "no valid provider entries"}
	}
	sortProviders(providers, nil)

	catRev := ""
	if s := parseString(root["catalogRevision"]); s != nil {
		catRev = *s
	} else if s := parseString(root["catalogHash"]); s != nil {
		catRev = *s
	} else if generatedAt != nil {
		catRev = generatedAt.UTC().Format(time.RFC3339)
	}

	totalModels := 0
	for _, p := range providers {
		totalModels += len(p.Models)
	}
	totalProviders := len(providers)
	if tp := parseInt64(root["totalProviders"]); tp != nil {
		totalProviders = int(*tp)
	}
	if tm := parseInt64(root["totalModels"]); tm != nil {
		totalModels = int(*tm)
	}

	var fetchedAt *time.Time
	if s := parseString(root["sourceFetchedAt"]); s != nil {
		fetchedAt = parseTimestamp(*s)
	}

	rawCopy := make([]byte, len(data))
	copy(rawCopy, data)

	return &Catalog{
		Revision: Revision{
			SchemaVersion:   schemaVersion,
			CatalogRevision: catRev,
			CatalogHash:     parseString(root["catalogHash"]),
			Source:          parseString(root["source"]),
			SourceURL:       parseString(root["sourceURL"]),
			SourceHash:      parseString(root["sourceHash"]),
			GeneratedAt:     generatedAt,
			SourceFetchedAt: fetchedAt,
			TotalProviders:  totalProviders,
			TotalModels:     totalModels,
		},
		Providers:   providers,
		RawDocument: rawCopy,
	}, nil
}

func decodeProvider(key string, fields rawObject) *Provider {
	modelsObj := parseObject(fields["models"])
	models := make([]Model, 0, len(modelsObj))
	for mid, rawModel := range modelsObj {
		mf := parseObject(rawModel)
		if mf == nil {
			continue
		}
		if m := decodeModel(key, mid, mf); m != nil {
			models = append(models, *m)
		}
	}
	if len(models) == 0 && isNullOrEmpty(fields["id"]) {
		return nil
	}

	id := key
	if s := parseString(fields["id"]); s != nil {
		id = *s
	}
	name := id
	if s := parseString(fields["name"]); s != nil {
		name = *s
	}

	return &Provider{
		ID:                       id,
		Name:                     name,
		BaseURL:                  nonEmptyString(parseString(fields["baseURL"])),
		DocumentationURL:         nonEmptyString(parseString(fields["doc"])),
		EnvironmentVariableNames: parseStringSlice(fields["env"]),
		Models:                   ModelsInStableOrder(models),
		Fields:                   fields,
	}
}

func decodeModel(providerID, key string, fields rawObject) *Model {
	idPtr := nonEmptyString(&key)
	if idPtr == nil {
		idPtr = nonEmptyString(parseString(fields["id"]))
	}
	if idPtr == nil {
		return nil
	}
	id := *idPtr
	name := id
	if s := parseString(fields["name"]); s != nil {
		name = *s
	}

	lim := parseObject(fields["limit"])
	cost := parseObject(fields["cost"])
	mod := parseObject(fields["modalities"])

	inMods := parseStringSlice(mod["input"])
	outMods := parseStringSlice(mod["output"])
	attachment := parseBoolValue(fields["attachment"])
	vision := hasModalityPrefix(inMods, "image") || attachment

	var efforts []string
	var rawOpts []json.RawMessage
	if !isNullOrEmpty(fields["reasoning_options"]) && json.Unmarshal(fields["reasoning_options"], &rawOpts) == nil {
		for _, opt := range rawOpts {
			if obj := parseObject(opt); obj != nil {
				efforts = append(efforts, parseStringSlice(obj["values"])...)
			}
		}
	}

	statusStr := ""
	if s := parseString(fields["status"]); s != nil {
		statusStr = *s
	}

	return &Model{
		ID:               id,
		ProviderID:       providerID,
		Name:             name,
		Family:           nonEmptyString(parseString(fields["family"])),
		Overview:         nonEmptyString(parseString(fields["description"])),
		ReleaseDate:      nonEmptyString(parseString(fields["release_date"])),
		LastUpdated:      nonEmptyString(parseString(fields["last_updated"])),
		KnowledgeCutoff:  nonEmptyString(parseString(fields["knowledge"])),
		CanonicalModelID: nonEmptyString(parseString(fields["canonical_model_id"])),
		Status:           ParseModelStatus(statusStr),
		Limits: Limits{
			Context: parseInt64(lim["context"]),
			Input:   parseInt64(lim["input"]),
			Output:  parseInt64(lim["output"]),
		},
		Capabilities: Capabilities{
			Reasoning:           parseBoolValue(fields["reasoning"]),
			ReasoningEfforts:    efforts,
			ToolCalling:         parseBoolValue(fields["tool_call"]),
			StructuredOutput:    parseBoolValue(fields["structured_output"]),
			Attachments:         attachment,
			Vision:              vision,
			AudioInput:          hasModalityPrefix(inMods, "audio"),
			AudioOutput:         hasModalityPrefix(outMods, "audio"),
			InputModalities:     inMods,
			OutputModalities:    outMods,
			OpenWeights:         parseBoolValue(fields["open_weights"]),
			SupportsTemperature: parseOptionalBool(fields["temperature"]),
		},
		Pricing: Pricing{
			Input:       parseFloat64(cost["input"]),
			Output:      parseFloat64(cost["output"]),
			CacheRead:   parseFloat64(cost["cache_read"]),
			CacheWrite:  parseFloat64(cost["cache_write"]),
			Reasoning:   parseFloat64(cost["reasoning"]),
			InputAudio:  parseFloat64(cost["input_audio"]),
			OutputAudio: parseFloat64(cost["output_audio"]),
		},
		Fields: fields,
	}
}

func isNullOrEmpty(b json.RawMessage) bool {
	trimmed := bytes.TrimSpace(b)
	return len(trimmed) == 0 || bytes.Equal(trimmed, []byte("null"))
}

func parseObject(b json.RawMessage) rawObject {
	trimmed := bytes.TrimSpace(b)
	if len(trimmed) == 0 || trimmed[0] != '{' {
		return nil
	}
	var m rawObject
	if json.Unmarshal(trimmed, &m) != nil {
		return nil
	}
	return m
}

func parseString(b json.RawMessage) *string {
	if isNullOrEmpty(b) {
		return nil
	}
	var s string
	if json.Unmarshal(b, &s) != nil {
		return nil
	}
	return &s
}

func nonEmptyString(s *string) *string {
	if s == nil || strings.TrimSpace(*s) == "" {
		return nil
	}
	return s
}

func parseInt64(b json.RawMessage) *int64 {
	if isNullOrEmpty(b) {
		return nil
	}
	var n int64
	if json.Unmarshal(b, &n) == nil {
		return &n
	}
	var f float64
	if json.Unmarshal(b, &f) == nil && f == float64(int64(f)) {
		v := int64(f)
		return &v
	}
	return nil
}

func parseFloat64(b json.RawMessage) *float64 {
	if isNullOrEmpty(b) {
		return nil
	}
	var f float64
	if json.Unmarshal(b, &f) != nil {
		return nil
	}
	return &f
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

func parseBoolValue(b json.RawMessage) bool {
	return string(bytes.TrimSpace(b)) == "true"
}

func parseStringSlice(b json.RawMessage) []string {
	if isNullOrEmpty(b) {
		return nil
	}
	var raw []json.RawMessage
	if json.Unmarshal(b, &raw) != nil {
		return nil
	}
	out := make([]string, 0, len(raw))
	for _, item := range raw {
		if s := parseString(item); s != nil {
			out = append(out, *s)
		}
	}
	return out
}

func hasModalityPrefix(modalities []string, prefix string) bool {
	for _, m := range modalities {
		if strings.HasPrefix(strings.ToLower(m), prefix) {
			return true
		}
	}
	return false
}

func parseTimestamp(text string) *time.Time {
	if text == "" {
		return nil
	}
	for _, layout := range []string{time.RFC3339Nano, time.RFC3339, "2006-01-02"} {
		if t, err := time.Parse(layout, text); err == nil {
			utc := t.UTC()
			return &utc
		}
	}
	return nil
}
