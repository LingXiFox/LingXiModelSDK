package main

import (
	"fmt"
	"net/url"
	"strconv"
	"strings"
)

// FilterOption defines a capability filter chip in the catalog UI.
type FilterOption struct {
	ID    string
	Label string
}

var catalogFilters = []FilterOption{
	{ID: "all", Label: "全部"},
	{ID: "reasoning", Label: "深度思考"},
	{ID: "vision", Label: "视觉输入"},
	{ID: "tools", Label: "工具调用"},
	{ID: "structured", Label: "结构化输出"},
	{ID: "largeContext", Label: "≥128k 窗口"},
}

// ProviderView holds display metadata for a provider.
type ProviderView struct {
	ID       string
	Name     string
	Endpoint string
	Doc      string
	Env      string
	Count    int
	Status   string
}

// ModelView holds display metadata for a model, keeping pointer fields so
// unknown (nil), zero (0), and false remain distinct.
type ModelView struct {
	ProviderID      string
	ProviderName    string
	ID              string
	Name            string
	Family          string
	Description     string
	Release         string
	Updated         string
	Status          string
	Context         string
	Output          string
	InputPrice      string
	OutputPrice     string
	PriceKnown      bool
	ContextN        *int64
	OutputN         *int64
	InputPriceN     *float64
	OutputPriceN    *float64
	Reasoning       *bool
	Vision          bool
	Attachments     *bool
	Tools           *bool
	Structured      *bool
	OpenWeights     *bool
	SupportsTemp    *bool
	InputModalities []string
}

func (m ModelView) IsReasoning() bool   { return m.Reasoning != nil && *m.Reasoning }
func (m ModelView) IsAttachments() bool { return m.Attachments != nil && *m.Attachments }
func (m ModelView) IsTools() bool       { return m.Tools != nil && *m.Tools }
func (m ModelView) IsStructured() bool  { return m.Structured != nil && *m.Structured }
func (m ModelView) IsOpenWeights() bool { return m.OpenWeights != nil && *m.OpenWeights }

// CatalogPageView is passed to templates/pages/catalog.html.
type CatalogPageView struct {
	NavActive   string
	PageTitle   string
	Catalog     Catalog
	Providers   []ProviderView
	Models      []ModelView
	Selected    *ProviderView
	Q, PID, Cap string
	Filters     []FilterOption
	Total       int
	Start       int
	End         int
	Page        int
	TotalPages  int
	Previous    string
	Next        string
	Unavailable bool
}

func (v CatalogPageView) Link(pid, cap string, page int) string {
	q := url.Values{}
	if pid != "" {
		q.Set("provider", pid)
	}
	if cap != "" && cap != "all" {
		q.Set("cap", cap)
	}
	if v.Q != "" {
		q.Set("q", v.Q)
	}
	if page > 1 {
		q.Set("page", strconv.Itoa(page))
	}
	if len(q) == 0 {
		return "/"
	}
	return "/?" + q.Encode()
}

// ProvidersPageView is passed to templates/pages/providers.html.
type ProvidersPageView struct {
	NavActive   string
	PageTitle   string
	Catalog     Catalog
	Providers   []ProviderView
	Q           string
	Total       int
	Start       int
	End         int
	Page        int
	TotalPages  int
	Previous    string
	Next        string
	Unavailable bool
}

func (v ProvidersPageView) Link(page int) string {
	q := url.Values{}
	if v.Q != "" {
		q.Set("q", v.Q)
	}
	if page > 1 {
		q.Set("page", strconv.Itoa(page))
	}
	if len(q) == 0 {
		return "/providers"
	}
	return "/providers?" + q.Encode()
}

// StaticPageView is passed to /sdk and /protocols.
type StaticPageView struct {
	NavActive string
	PageTitle string
	Catalog   Catalog
	HasSnap   bool
}

func matchesCatalogFilter(m ModelView, v CatalogPageView) bool {
	if v.PID != "" && v.PID != m.ProviderID {
		return false
	}
	switch v.Cap {
	case "reasoning":
		if !m.IsReasoning() {
			return false
		}
	case "vision":
		if !m.Vision {
			return false
		}
	case "tools":
		if !m.IsTools() {
			return false
		}
	case "structured":
		if !m.IsStructured() {
			return false
		}
	case "largeContext":
		if m.ContextN == nil || *m.ContextN < 128000 {
			return false
		}
	}
	if v.Q == "" {
		return true
	}
	q := strings.ToLower(v.Q)
	haystack := strings.ToLower(strings.Join([]string{m.ID, m.Name, m.Family, m.ProviderID, m.ProviderName}, " "))
	return strings.Contains(haystack, q)
}

func buildCatalogView(s *Snapshot, q url.Values) CatalogPageView {
	v := CatalogPageView{
		NavActive: "catalog",
		PageTitle: "LingXi Models — 公共模型目录",
		Q:         strings.TrimSpace(q.Get("q")),
		PID:       q.Get("provider"),
		Cap:       q.Get("cap"),
		Filters:   catalogFilters,
		Page:      1,
	}
	if len(v.Q) > 512 {
		v.Q = v.Q[:512]
	}
	validCap := false
	for _, f := range catalogFilters {
		if v.Cap == f.ID {
			validCap = true
			break
		}
	}
	if !validCap {
		v.Cap = "all"
	}
	if s == nil {
		v.Unavailable = true
		return v
	}

	v.Catalog = s.Catalog
	v.Providers = s.Providers
	for i := range s.Providers {
		if s.Providers[i].ID == v.PID {
			v.Selected = &s.Providers[i]
			break
		}
	}
	if v.Selected == nil {
		v.PID = ""
	}

	const pageSize = 60
	filtered := make([]ModelView, 0, len(s.Models))
	for _, m := range s.Models {
		if matchesCatalogFilter(m, v) {
			filtered = append(filtered, m)
		}
	}

	v.Total = len(filtered)
	v.TotalPages = (v.Total + pageSize - 1) / pageSize
	if v.TotalPages < 1 {
		v.TotalPages = 1
	}
	if n, err := strconv.Atoi(q.Get("page")); err == nil && n > 0 {
		v.Page = n
	}
	if v.Page > v.TotalPages {
		v.Page = v.TotalPages
	}
	start := (v.Page - 1) * pageSize
	end := start + pageSize
	if end > v.Total {
		end = v.Total
	}
	v.Models = filtered[start:end]
	v.End = end
	if end > 0 {
		v.Start = start + 1
	}
	if v.Page > 1 {
		v.Previous = v.Link(v.PID, v.Cap, v.Page-1)
	}
	if v.Page < v.TotalPages {
		v.Next = v.Link(v.PID, v.Cap, v.Page+1)
	}
	return v
}

func buildProvidersView(s *Snapshot, q url.Values) ProvidersPageView {
	v := ProvidersPageView{
		NavActive: "providers",
		PageTitle: "提供商验证状态 — LingXi Models",
		Q:         strings.TrimSpace(q.Get("q")),
		Page:      1,
	}
	if len(v.Q) > 512 {
		v.Q = v.Q[:512]
	}
	if s == nil {
		v.Unavailable = true
		return v
	}
	v.Catalog = s.Catalog

	query := strings.ToLower(v.Q)
	filtered := make([]ProviderView, 0, len(s.Providers))
	for _, p := range s.Providers {
		if query != "" {
			haystack := strings.ToLower(p.ID + " " + p.Name + " " + p.Endpoint)
			if !strings.Contains(haystack, query) {
				continue
			}
		}
		filtered = append(filtered, p)
	}

	const pageSize = 60
	v.Total = len(filtered)
	v.TotalPages = (v.Total + pageSize - 1) / pageSize
	if v.TotalPages < 1 {
		v.TotalPages = 1
	}
	if n, err := strconv.Atoi(q.Get("page")); err == nil && n > 0 {
		v.Page = n
	}
	if v.Page > v.TotalPages {
		v.Page = v.TotalPages
	}
	start := (v.Page - 1) * pageSize
	end := start + pageSize
	if end > v.Total {
		end = v.Total
	}
	v.Providers = filtered[start:end]
	v.End = end
	if end > 0 {
		v.Start = start + 1
	}
	if v.Page > 1 {
		v.Previous = v.Link(v.Page - 1)
	}
	if v.Page < v.TotalPages {
		v.Next = v.Link(v.Page + 1)
	}
	return v
}

func buildStaticView(s *Snapshot, nav, title string) StaticPageView {
	v := StaticPageView{
		NavActive: nav,
		PageTitle: title,
	}
	if s != nil {
		v.Catalog = s.Catalog
		v.HasSnap = true
	}
	return v
}

func fallbackString(s, other string) string {
	if s == "" {
		return other
	}
	return s
}

func formatTokenLimit(n *int64) string {
	if n == nil {
		return "—"
	}
	val := *n
	if val >= 1_000_000 {
		if val%1_000_000 == 0 {
			return fmt.Sprintf("%dM", val/1_000_000)
		}
		return fmt.Sprintf("%.1fM", float64(val)/1_000_000)
	}
	if val >= 1_000 {
		return fmt.Sprintf("%dk", val/1_000)
	}
	return strconv.FormatInt(val, 10)
}

func formatPriceUSD(p *float64) string {
	if p == nil {
		return "—"
	}
	s := fmt.Sprintf("%.8f", *p)
	s = strings.TrimRight(s, "0")
	s = strings.TrimRight(s, ".")
	if s == "" || s == "-0" {
		s = "0"
	}
	return "$" + s
}
