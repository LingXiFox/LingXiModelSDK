package native

import (
	"encoding/json"
	"fmt"
	"net/url"
	"strings"
)

// Protocol identifies a native upstream wire protocol.
type Protocol string

const (
	ProtocolOpenAIChatCompletions Protocol = "openai_chat_completions"
	ProtocolOpenAIResponses       Protocol = "openai_responses"
	ProtocolAnthropicMessages     Protocol = "anthropic_messages"
	ProtocolGoogleGemini          Protocol = "google_gemini"
)

// AuthConfig describes how a provider authenticates requests.
type AuthConfig struct {
	Type       string   `json:"type"`
	HeaderName string   `json:"headerName,omitempty"`
	Prefix     string   `json:"prefix,omitempty"`
	QueryParam string   `json:"queryParam,omitempty"`
	Env        []string `json:"env,omitempty"`
}

// ModelOverride holds model-specific overrides in the registry.
type ModelOverride struct {
	Endpoint       string            `json:"endpoint,omitempty"`
	Protocols      []string          `json:"protocols,omitempty"`
	DefaultHeaders map[string]string `json:"defaultHeaders,omitempty"`
	ExtraParams    map[string]any    `json:"extraParams,omitempty"`
}

// ProviderEntry mirrors one entry in registry/registry.json.
type ProviderEntry struct {
	ID                 string                   `json:"id"`
	Name               string                   `json:"name"`
	Protocols          []string                 `json:"protocols"`
	OfficialEndpoints  map[string]string        `json:"officialEndpoints"`
	Auth               AuthConfig               `json:"auth"`
	DefaultHeaders     map[string]string        `json:"defaultHeaders"`
	ExtraParams        map[string]any           `json:"extraParams"`
	ModelOverrides     map[string]ModelOverride `json:"modelOverrides"`
	VerificationStatus string                   `json:"verificationStatus"`
}

// Registry holds declarative provider definitions.
type Registry struct {
	providers map[string]ProviderEntry
}

// ParseRegistry decodes registry/registry.json bytes.
func ParseRegistry(data []byte) (*Registry, error) {
	var doc struct {
		SchemaVersion string                   `json:"schemaVersion"`
		Providers     map[string]ProviderEntry `json:"providers"`
	}
	if err := json.Unmarshal(data, &doc); err != nil {
		return nil, fmt.Errorf("invalid registry JSON: %w", err)
	}
	if doc.SchemaVersion != "1.0" {
		return nil, fmt.Errorf("unsupported registry schemaVersion %q", doc.SchemaVersion)
	}
	return &Registry{providers: doc.Providers}, nil
}

// Len returns the number of registered providers.
func (r *Registry) Len() int {
	return len(r.providers)
}

// Get returns a provider entry by ID.
func (r *Registry) Get(providerID string) (ProviderEntry, bool) {
	entry, ok := r.providers[providerID]
	return entry, ok
}

// ResolveEndpoint validates protocol support, resolves the base URL, and checks HTTPS safety.
func (r *Registry) ResolveEndpoint(providerID string, proto Protocol, modelID, endpointOverride string, allowInsecureHTTP bool) (ProviderEntry, string, error) {
	entry, ok := r.Get(providerID)
	if !ok {
		return ProviderEntry{}, "", fmt.Errorf("unknown provider %q", providerID)
	}
	supported := false
	for _, p := range entry.Protocols {
		if p == string(proto) {
			supported = true
			break
		}
	}
	if !supported {
		return ProviderEntry{}, "", fmt.Errorf("provider %q does not declare protocol %q", providerID, proto)
	}

	base := endpointOverride
	if base == "" && modelID != "" {
		if ov, has := entry.ModelOverrides[modelID]; has && ov.Endpoint != "" {
			base = ov.Endpoint
		}
	}
	if base == "" {
		base = entry.OfficialEndpoints[string(proto)]
	}
	if base == "" {
		return ProviderEntry{}, "", fmt.Errorf("no endpoint configured for %s/%s", providerID, proto)
	}
	base = strings.TrimRight(base, "/")
	if err := ValidateEndpointURL(base, allowInsecureHTTP); err != nil {
		return ProviderEntry{}, "", err
	}
	return entry, base, nil
}

// ValidateEndpointURL enforces HTTPS for non-loopback hosts unless allowInsecureHTTP is set.
func ValidateEndpointURL(rawURL string, allowInsecureHTTP bool) error {
	u, err := url.Parse(rawURL)
	if err != nil {
		return fmt.Errorf("invalid endpoint URL %q: %w", rawURL, err)
	}
	switch u.Scheme {
	case "https":
		return nil
	case "http":
		host := u.Hostname()
		if host == "127.0.0.1" || host == "localhost" || host == "::1" || allowInsecureHTTP {
			return nil
		}
		return fmt.Errorf("non-HTTPS endpoint %q is disallowed for non-loopback hosts", rawURL)
	default:
		return fmt.Errorf("unsupported URL scheme %q", u.Scheme)
	}
}
