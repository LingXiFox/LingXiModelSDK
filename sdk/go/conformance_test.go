package catalog_test

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	catalog "github.com/LingXiFox/LingXiModelSDK/sdk/go"
)

type expectations struct {
	SchemaVersion         string   `json:"schemaVersion"`
	CatalogRevision       string   `json:"catalogRevision"`
	TotalProviders        int      `json:"totalProviders"`
	TotalModels           int      `json:"totalModels"`
	ProviderOrder         []string `json:"providerOrder"`
	FeaturedProviderOrder struct {
		Featured []string `json:"featured"`
		Expected []string `json:"expected"`
	} `json:"featuredProviderOrder"`
	Prov10ModelOrder        []string            `json:"prov10ModelOrder"`
	DeepSeekModelOrder      []string            `json:"deepseekModelOrder"`
	AllModelQualifiedOrder  []string            `json:"allModelQualifiedOrder"`
	Search                  map[string][]string `json:"search"`
	Filters                 map[string][]string `json:"filters"`
}

func readFixture(t *testing.T, name string) []byte {
	t.Helper()
	b, err := os.ReadFile(filepath.Join("..", "..", "conformance", "catalog", "fixtures", name))
	if err != nil {
		t.Fatal(err)
	}
	return b
}

func TestConformanceV2(t *testing.T) {
	expBytes, err := os.ReadFile(filepath.Join("..", "..", "conformance", "catalog", "expectations.json"))
	if err != nil {
		t.Fatal(err)
	}
	var exp expectations
	if err := json.Unmarshal(expBytes, &exp); err != nil {
		t.Fatal(err)
	}

	cat, err := catalog.Decode(readFixture(t, "v2_catalog.json"))
	if err != nil {
		t.Fatal(err)
	}
	if cat.Revision.SchemaVersion != exp.SchemaVersion || cat.Revision.CatalogRevision != exp.CatalogRevision {
		t.Fatalf("unexpected revision: %+v", cat.Revision)
	}
	if cat.Revision.TotalProviders != exp.TotalProviders || cat.TotalModels() != exp.TotalModels {
		t.Fatalf("unexpected totals: %d/%d", cat.Revision.TotalProviders, cat.TotalModels())
	}

	var provIDs []string
	for _, p := range cat.SortedProviders() {
		provIDs = append(provIDs, p.ID)
	}
	if !reflect.DeepEqual(provIDs, exp.ProviderOrder) {
		t.Fatalf("provider order: got %v want %v", provIDs, exp.ProviderOrder)
	}

	var featIDs []string
	for _, p := range cat.SortedProviders(exp.FeaturedProviderOrder.Featured...) {
		featIDs = append(featIDs, p.ID)
	}
	if !reflect.DeepEqual(featIDs, exp.FeaturedProviderOrder.Expected) {
		t.Fatalf("featured order: got %v want %v", featIDs, exp.FeaturedProviderOrder.Expected)
	}

	checkModels := func(got []catalog.Model, want []string, label string) {
		t.Helper()
		var ids []string
		for _, m := range got {
			ids = append(ids, m.ID)
		}
		if !reflect.DeepEqual(ids, want) {
			t.Fatalf("%s: got %v want %v", label, ids, want)
		}
	}
	checkQualified := func(got []catalog.Model, want []string, label string) {
		t.Helper()
		var ids []string
		for _, m := range got {
			ids = append(ids, m.QualifiedID())
		}
		if !reflect.DeepEqual(ids, want) {
			t.Fatalf("%s: got %v want %v", label, ids, want)
		}
	}

	checkModels(cat.ModelsForProvider("prov10"), exp.Prov10ModelOrder, "prov10 models")
	checkModels(cat.ModelsForProvider("deepseek"), exp.DeepSeekModelOrder, "deepseek models")
	checkQualified(cat.AllModels(), exp.AllModelQualifiedOrder, "all models")

	gpt10, ok := cat.ModelInProvider("PROV10", "GPT-10")
	if !ok {
		t.Fatal("missing gpt-10")
	}
	if gpt10.Pricing.Input == nil || *gpt10.Pricing.Input != 0.0 {
		t.Fatalf("expected $0 input price, got %v", gpt10.Pricing.Input)
	}
	if gpt10.Capabilities.SupportsTemperature == nil || *gpt10.Capabilities.SupportsTemperature != false {
		t.Fatalf("expected temperature=false, got %v", gpt10.Capabilities.SupportsTemperature)
	}
	if !gpt10.Capabilities.Vision || !gpt10.Capabilities.AudioInput || !gpt10.Capabilities.AudioOutput {
		t.Fatalf("unexpected capabilities: %+v", gpt10.Capabilities)
	}
	if string(gpt10.Fields["experimental"]) == "" {
		t.Fatal("missing unknown field experimental")
	}

	gpt5, ok := cat.Model("prov10/gpt-5")
	if !ok {
		t.Fatal("missing prov10/gpt-5")
	}
	if gpt5.Limits.Context == nil || *gpt5.Limits.Context != 0 {
		t.Fatalf("expected context=0, got %v", gpt5.Limits.Context)
	}
	if !gpt5.Pricing.IsUnknown() || gpt5.Capabilities.SupportsTemperature != nil {
		t.Fatalf("expected unknown pricing and nil temperature: %+v", gpt5)
	}

	checkQualified(cat.Search("gpt", 1), exp.Search["gptLimit1"], "search gpt limit 1")
	checkQualified(cat.Search("deepseek", 0), exp.Search["deepseek"], "search deepseek")

	trueVal := true
	minCtx := int64(200000)
	ds := "deepseek"
	checkQualified(cat.ModelsMatching(catalog.ModelFilter{Vision: &trueVal}), exp.Filters["vision"], "filter vision")
	checkQualified(cat.ModelsMatching(catalog.ModelFilter{MinimumContextWindow: &minCtx}), exp.Filters["minContext200k"], "filter 200k")
	checkQualified(cat.ModelsMatching(catalog.ModelFilter{ProviderID: &ds}), exp.Filters["deepseekDefault"], "filter deepseek default")
	checkQualified(cat.ModelsMatching(catalog.ModelFilter{ProviderID: &ds, IncludeDeprecated: true}), exp.Filters["deepseekIncludeDeprecated"], "filter deepseek deprecated")
}

func TestConformanceErrorAndLegacyFixtures(t *testing.T) {
	legacy, err := catalog.Decode(readFixture(t, "v1_legacy.json"))
	if err != nil || legacy.Revision.SchemaVersion != "1.0" {
		t.Fatalf("legacy decode failed: %v", err)
	}

	malformed, err := catalog.Decode(readFixture(t, "malformed_entry.json"))
	if err != nil {
		t.Fatalf("malformed decode failed: %v", err)
	}
	if _, ok := malformed.ModelInProvider("p", "published-key"); !ok {
		t.Fatal("expected published-key model")
	}
	if _, ok := malformed.ModelInProvider("p", "internal-id"); ok {
		t.Fatal("internal-id should not match")
	}

	_, err = catalog.Decode(readFixture(t, "unsupported_schema.json"))
	var ce *catalog.CatalogError
	if !errors.As(err, &ce) || ce.Kind != catalog.ErrUnsupportedSchemaVersion || ce.Version != "3.0" {
		t.Fatalf("expected unsupported schema 3.0 error, got %v", err)
	}

	if _, err := catalog.Decode(readFixture(t, "broken_empty_providers.json")); err == nil {
		t.Fatal("expected error for empty providers")
	}
}
