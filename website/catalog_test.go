package main

import (
	"bytes"
	"testing"
	"time"
)

const richFixture = `{
  "p2": {
    "name": "Provider 10",
    "api": "https://api.example.com/v1",
    "env": ["KEY_10"],
    "unknownProv": {"nested": [1, 2]},
    "swiftDriver": "legacy-guess",
    "models": {
      "m1": {
        "name": "GPT-10",
        "release_date": "2026-01-01",
        "limit": {"context": 200000},
        "cost": {"input": 0, "output": 0.000001},
        "reasoning": true,
        "attachment": true,
        "modalities": {"input": ["text"]},
        "extraNested": {"b": 2, "a": 1},
        "swiftSnippet": "remove-me"
      },
      "m2": {
        "name": "GPT-5",
        "release_date": "2026-01-01",
        "limit": {"context": 0, "output": 4096},
        "structured_output": false,
        "modalities": {"input": ["text", "image"]}
      }
    }
  },
  "p1": {
    "name": "Provider 2",
    "models": {
      "undated": {
        "name": "Alpha"
      }
    }
  }
}`

func buildTestSnapshot(t *testing.T, body string) *Snapshot {
	t.Helper()
	out, _, err := normalize([]byte(body), defaultSourceURL, time.Unix(1700000000, 0).UTC(), nil, defaultMinRatio, false)
	if err != nil {
		t.Fatalf("normalize failed: %v", err)
	}
	snap, err := parseSnapshot(out, "")
	if err != nil {
		t.Fatalf("parseSnapshot failed: %v", err)
	}
	return snap
}

func TestNormalizePreservesUnknownAndStripsGuesses(t *testing.T) {
	snap := buildTestSnapshot(t, richFixture)

	if snap.Catalog.TotalProviders != 2 || snap.Catalog.TotalModels != 3 {
		t.Fatalf("unexpected totals: providers=%d models=%d", snap.Catalog.TotalProviders, snap.Catalog.TotalModels)
	}
	if !bytes.Contains(snap.Raw, []byte(`"unknownProv"`)) || !bytes.Contains(snap.Raw, []byte(`"extraNested"`)) {
		t.Fatalf("unknown fields were dropped: %s", snap.Raw)
	}
	if bytes.Contains(snap.Raw, []byte("swiftDriver")) || bytes.Contains(snap.Raw, []byte("swiftSnippet")) {
		t.Fatalf("runtime guess fields were not stripped")
	}
}

func TestThreeStateValueSemantics(t *testing.T) {
	snap := buildTestSnapshot(t, richFixture)

	var m1, m2, undated ModelView
	for _, m := range snap.Models {
		switch m.ID {
		case "m1":
			m1 = m
		case "m2":
			m2 = m
		case "undated":
			undated = m
		}
	}

	// Zero price ($0) is distinct from unknown price (nil / PriceKnown == false).
	if !m1.PriceKnown || m1.InputPrice != "$0" || m1.OutputPrice != "$0.000001" {
		t.Fatalf("unexpected m1 pricing: %+v", m1)
	}
	if m1.OutputN != nil || m1.Output != "—" {
		t.Fatalf("expected unknown output limit for m1, got %v / %q", m1.OutputN, m1.Output)
	}
	if m1.Vision || !m1.IsAttachments() {
		t.Fatalf("m1 attachment should not imply vision: vision=%v attachments=%v", m1.Vision, m1.IsAttachments())
	}

	// Zero context limit (0) is distinct from nil ("—").
	if m2.ContextN == nil || *m2.ContextN != 0 || m2.Context != "0" {
		t.Fatalf("expected zero context limit preserved, got %v / %q", m2.ContextN, m2.Context)
	}
	if m2.Structured == nil || *m2.Structured != false {
		t.Fatalf("expected explicit false structured_output preserved, got %v", m2.Structured)
	}
	if !m2.Vision {
		t.Fatalf("expected vision=true for image input modality")
	}

	if undated.PriceKnown || undated.InputPrice != "—" || undated.Context != "—" {
		t.Fatalf("expected unknown pricing and limits for undated model, got %+v", undated)
	}
	if undated.Reasoning != nil {
		t.Fatalf("expected nil reasoning when omitted, got %v", *undated.Reasoning)
	}
}

func TestCanonicalHashDeterminismWithShuffledNestedKeys(t *testing.T) {
	a := `{"p":{"name":"P","models":{"m":{"name":"M","extra":{"z":1,"a":{"y":2,"x":3}}}}}}`
	b := `{"p":{"models":{"m":{"extra":{"a":{"x":3,"y":2},"z":1},"name":"M"}},"name":"P"}}`

	outA, _, err := normalize([]byte(a), defaultSourceURL, time.Unix(1, 0), nil, defaultMinRatio, false)
	if err != nil {
		t.Fatal(err)
	}
	outB, _, err := normalize([]byte(b), defaultSourceURL, time.Unix(200, 0), nil, defaultMinRatio, false)
	if err != nil {
		t.Fatal(err)
	}

	snapA, err := parseSnapshot(outA, "")
	if err != nil {
		t.Fatal(err)
	}
	snapB, err := parseSnapshot(outB, "")
	if err != nil {
		t.Fatal(err)
	}
	if snapA.Catalog.Revision != snapB.Catalog.Revision || snapA.Catalog.Hash != snapB.Catalog.Hash {
		t.Fatalf("canonical hash drifted: %s vs %s", snapA.Catalog.Hash, snapB.Catalog.Hash)
	}
}

func TestNaturalOrderingParity(t *testing.T) {
	if naturalCompare("Provider 2", "Provider 10") >= 0 {
		t.Fatalf("expected Provider 2 < Provider 10")
	}
	if naturalCompare("GPT-5", "GPT-10") >= 0 {
		t.Fatalf("expected GPT-5 < GPT-10")
	}
	// Numbers sort before text tokens at the same position (matching Swift ModelCatalogOrdering).
	if naturalCompare("2fa", "alpha") >= 0 {
		t.Fatalf("expected number token to sort before text token")
	}

	snap := buildTestSnapshot(t, richFixture)
	if snap.Providers[0].ID != "p1" || snap.Providers[1].ID != "p2" {
		t.Fatalf("unexpected provider sort order: %+v", snap.Providers)
	}
	// Under p2: GPT-5 (m2) must precede GPT-10 (m1) when release_date matches.
	var p2Models []string
	for _, m := range snap.Models {
		if m.ProviderID == "p2" {
			p2Models = append(p2Models, m.ID)
		}
	}
	if len(p2Models) != 2 || p2Models[0] != "m2" || p2Models[1] != "m1" {
		t.Fatalf("unexpected model sort order under p2: %v", p2Models)
	}
}
