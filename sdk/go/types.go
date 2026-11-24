package catalog

import (
	"encoding/json"
	"time"
)

// ModelStatus represents the lifecycle state of a published model.
type ModelStatus string

const (
	StatusActive     ModelStatus = "active"
	StatusPreview    ModelStatus = "preview"
	StatusDeprecated ModelStatus = "deprecated"
	StatusRetired    ModelStatus = "retired"
	StatusUnknown    ModelStatus = "unknown"
)

// ParseModelStatus maps raw status strings leniently to ModelStatus.
func ParseModelStatus(raw string) ModelStatus {
	switch raw {
	case "deprecated", "retired":
		return StatusDeprecated
	case "beta", "preview":
		return StatusPreview
	case "active":
		return StatusActive
	default:
		return StatusUnknown
	}
}

// IsSelectable returns true unless the model is deprecated or retired.
func (s ModelStatus) IsSelectable() bool {
	return s != StatusDeprecated && s != StatusRetired
}

// Limits describes token limits; nil fields mean unknown/omitted, not zero.
type Limits struct {
	Context *int64
	Input   *int64
	Output  *int64
}

// Capabilities describes model capabilities as published.
type Capabilities struct {
	Reasoning           bool
	ReasoningEfforts    []string
	ToolCalling         bool
	StructuredOutput    bool
	Attachments         bool
	Vision              bool
	AudioInput          bool
	AudioOutput         bool
	InputModalities     []string
	OutputModalities    []string
	OpenWeights         bool
	SupportsTemperature *bool
}

// Pricing describes per-1M-token USD prices; nil means unknown, 0 means free.
type Pricing struct {
	Input       *float64
	Output      *float64
	CacheRead   *float64
	CacheWrite  *float64
	Reasoning   *float64
	InputAudio  *float64
	OutputAudio *float64
}

// IsUnknown returns true when no pricing fields are stated.
func (p Pricing) IsUnknown() bool {
	return p.Input == nil &&
		p.Output == nil &&
		p.CacheRead == nil &&
		p.CacheWrite == nil &&
		p.Reasoning == nil &&
		p.InputAudio == nil &&
		p.OutputAudio == nil
}

// Model represents a single published model with unknown fields preserved.
type Model struct {
	ID               string
	ProviderID       string
	Name             string
	Family           *string
	Overview         *string
	ReleaseDate      *string
	LastUpdated      *string
	KnowledgeCutoff  *string
	CanonicalModelID *string
	Status           ModelStatus
	Limits           Limits
	Capabilities     Capabilities
	Pricing          Pricing
	Fields           map[string]json.RawMessage
}

// QualifiedID returns "providerID/id".
func (m Model) QualifiedID() string {
	return m.ProviderID + "/" + m.ID
}

// Provider represents a single published provider with unknown fields preserved.
type Provider struct {
	ID                       string
	Name                     string
	BaseURL                  *string
	DocumentationURL         *string
	EnvironmentVariableNames []string
	Models                   []Model
	Fields                   map[string]json.RawMessage
}

// Revision describes catalog provenance and freshness metadata.
type Revision struct {
	SchemaVersion   string
	CatalogRevision string
	CatalogHash     *string
	Source          *string
	SourceURL       *string
	SourceHash      *string
	GeneratedAt     *time.Time
	SourceFetchedAt *time.Time
	TotalProviders  int
	TotalModels     int
}

// Catalog is an immutable snapshot of the LingXi public model catalog.
type Catalog struct {
	Revision    Revision
	Providers   []Provider
	RawDocument []byte
}
