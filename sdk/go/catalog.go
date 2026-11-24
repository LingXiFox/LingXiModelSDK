package catalog

import "strings"

// ModelFilter constrains Catalog.ModelsMatching. Nil fields impose no constraint.
type ModelFilter struct {
	ProviderID             *string
	Query                  *string
	Reasoning              *bool
	ToolCalling            *bool
	StructuredOutput       *bool
	Vision                 *bool
	Attachments            *bool
	OpenWeights            *bool
	MinimumContextWindow   *int64
	MinimumMaxOutputTokens *int64
	IncludeDeprecated      bool
	FeaturedProviderIDs    []string
}

func normalizeKey(s string) string {
	return strings.ToLower(strings.TrimSpace(s))
}

// TotalModels returns the total number of models across all providers.
func (c *Catalog) TotalModels() int {
	total := 0
	for _, p := range c.Providers {
		total += len(p.Models)
	}
	return total
}

// Provider looks up a provider by ID (case-insensitive, trimmed).
func (c *Catalog) Provider(providerID string) (Provider, bool) {
	key := normalizeKey(providerID)
	for _, p := range c.Providers {
		if normalizeKey(p.ID) == key {
			return p, true
		}
	}
	return Provider{}, false
}

// ModelInProvider looks up a model by provider ID and model ID.
func (c *Catalog) ModelInProvider(providerID, modelID string) (Model, bool) {
	p, ok := c.Provider(providerID)
	if !ok {
		return Model{}, false
	}
	key := normalizeKey(modelID)
	for _, m := range p.Models {
		if normalizeKey(m.ID) == key {
			return m, true
		}
	}
	return Model{}, false
}

// Model looks up a model by "provider/model" or bare model ID.
func (c *Catalog) Model(id string) (Model, bool) {
	if pid, mid, ok := strings.Cut(id, "/"); ok {
		return c.ModelInProvider(pid, mid)
	}
	key := normalizeKey(id)
	for _, p := range c.Providers {
		for _, m := range p.Models {
			if normalizeKey(m.ID) == key {
				return m, true
			}
		}
	}
	return Model{}, false
}

// SortedProviders returns providers sorted by featured rank, display name, and ID.
func (c *Catalog) SortedProviders(featured ...string) []Provider {
	out := make([]Provider, len(c.Providers))
	copy(out, c.Providers)
	sortProviders(out, featured)
	return out
}

// ModelsForProvider returns models for a provider in deterministic display order.
func (c *Catalog) ModelsForProvider(providerID string) []Model {
	p, ok := c.Provider(providerID)
	if !ok {
		return nil
	}
	return ModelsInStableOrder(p.Models)
}

// AllModels returns all models grouped by provider display order.
func (c *Catalog) AllModels() []Model {
	out := make([]Model, 0, c.TotalModels())
	for _, p := range c.SortedProviders() {
		out = append(out, ModelsInStableOrder(p.Models)...)
	}
	return out
}

// Search matches query against provider ID/name or model ID/name/family/overview.
func (c *Catalog) Search(query string, limit int) []Model {
	needle := normalizeKey(query)
	if needle == "" {
		all := c.AllModels()
		if limit > 0 && len(all) > limit {
			return all[:limit]
		}
		return all
	}

	var matches []Model
	for _, p := range c.SortedProviders() {
		ordered := ModelsInStableOrder(p.Models)
		if strings.Contains(normalizeKey(p.ID), needle) || strings.Contains(normalizeKey(p.Name), needle) {
			matches = append(matches, ordered...)
		} else {
			for _, m := range ordered {
				if containsAny(needle, m.ID, m.Name, derefStr(m.Family), derefStr(m.Overview)) {
					matches = append(matches, m)
				}
			}
		}
		if limit > 0 && len(matches) >= limit {
			return matches[:limit]
		}
	}
	return matches
}

// ModelsMatching filters models by published metadata.
func (c *Catalog) ModelsMatching(filter ModelFilter) []Model {
	var matches []Model
	for _, p := range c.SortedProviders(filter.FeaturedProviderIDs...) {
		if filter.ProviderID != nil && normalizeKey(p.ID) != normalizeKey(*filter.ProviderID) {
			continue
		}
		for _, m := range ModelsInStableOrder(p.Models) {
			if filterMatches(m, p, filter) {
				matches = append(matches, m)
			}
		}
	}
	return matches
}

func filterMatches(m Model, p Provider, f ModelFilter) bool {
	if !f.IncludeDeprecated && !m.Status.IsSelectable() {
		return false
	}
	if f.Reasoning != nil && m.Capabilities.Reasoning != *f.Reasoning {
		return false
	}
	if f.ToolCalling != nil && m.Capabilities.ToolCalling != *f.ToolCalling {
		return false
	}
	if f.StructuredOutput != nil && m.Capabilities.StructuredOutput != *f.StructuredOutput {
		return false
	}
	if f.Vision != nil && m.Capabilities.Vision != *f.Vision {
		return false
	}
	if f.Attachments != nil && m.Capabilities.Attachments != *f.Attachments {
		return false
	}
	if f.OpenWeights != nil && m.Capabilities.OpenWeights != *f.OpenWeights {
		return false
	}
	if f.MinimumContextWindow != nil {
		var ctx int64
		if m.Limits.Context != nil {
			ctx = *m.Limits.Context
		}
		if ctx < *f.MinimumContextWindow {
			return false
		}
	}
	if f.MinimumMaxOutputTokens != nil {
		var out int64
		if m.Limits.Output != nil {
			out = *m.Limits.Output
		}
		if out < *f.MinimumMaxOutputTokens {
			return false
		}
	}
	if f.Query != nil {
		q := normalizeKey(*f.Query)
		if q != "" && !containsAny(q, m.ID, m.Name, derefStr(m.Family), derefStr(m.Overview), p.ID, p.Name) {
			return false
		}
	}
	return true
}

func containsAny(needle string, haystacks ...string) bool {
	for _, h := range haystacks {
		if strings.Contains(normalizeKey(h), needle) {
			return true
		}
	}
	return false
}

func derefStr(s *string) string {
	if s == nil {
		return ""
	}
	return *s
}
