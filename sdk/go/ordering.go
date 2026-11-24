package catalog

import (
	"sort"
	"strings"
)

type naturalToken struct {
	isNumber bool
	raw      string
	trimmed  string
}

func tokenizeNatural(text string) []naturalToken {
	if text == "" {
		return nil
	}
	var tokens []naturalToken
	var digits strings.Builder
	var words strings.Builder

	flushDigits := func() {
		if digits.Len() == 0 {
			return
		}
		raw := digits.String()
		trimmed := strings.TrimLeft(raw, "0")
		if trimmed == "" {
			trimmed = "0"
		}
		tokens = append(tokens, naturalToken{isNumber: true, raw: raw, trimmed: trimmed})
		digits.Reset()
	}

	flushWords := func() {
		if words.Len() == 0 {
			return
		}
		tokens = append(tokens, naturalToken{isNumber: false, raw: strings.ToLower(words.String())})
		words.Reset()
	}

	for _, r := range text {
		if r >= '0' && r <= '9' {
			flushWords()
			digits.WriteRune(r)
		} else {
			flushDigits()
			words.WriteRune(r)
		}
	}
	flushDigits()
	flushWords()
	return tokens
}

// CompareNatural compares two strings using digit-aware natural ordering.
// Numbers sort before text at the same token position.
func CompareNatural(a, b string) int {
	left := tokenizeNatural(a)
	right := tokenizeNatural(b)
	limit := len(left)
	if len(right) < limit {
		limit = len(right)
	}
	for i := 0; i < limit; i++ {
		lt, rt := left[i], right[i]
		switch {
		case lt.isNumber && !rt.isNumber:
			return -1
		case !lt.isNumber && rt.isNumber:
			return 1
		case lt.isNumber && rt.isNumber:
			if len(lt.trimmed) != len(rt.trimmed) {
				if len(lt.trimmed) < len(rt.trimmed) {
					return -1
				}
				return 1
			}
			if lt.trimmed != rt.trimmed {
				if lt.trimmed < rt.trimmed {
					return -1
				}
				return 1
			}
			if lt.raw != rt.raw {
				if lt.raw < rt.raw {
					return -1
				}
				return 1
			}
		default:
			if lt.raw != rt.raw {
				if lt.raw < rt.raw {
					return -1
				}
				return 1
			}
		}
	}
	if len(left) < len(right) {
		return -1
	}
	if len(left) > len(right) {
		return 1
	}
	return 0
}

func sortProviders(providers []Provider, featured []string) {
	ranks := make(map[string]int, len(featured))
	for i, id := range featured {
		ranks[normalizeKey(id)] = i
	}
	sort.SliceStable(providers, func(i, j int) bool {
		a, b := providers[i], providers[j]
		ra, oka := ranks[normalizeKey(a.ID)]
		rb, okb := ranks[normalizeKey(b.ID)]
		if oka != okb {
			return oka
		}
		if oka && ra != rb {
			return ra < rb
		}
		if cmp := CompareNatural(a.Name, b.Name); cmp != 0 {
			return cmp < 0
		}
		return a.ID < b.ID
	})
}

// ModelsInStableOrder returns a sorted copy of models: release_date DESC (dated before undated),
// then family, then name, then id.
func ModelsInStableOrder(models []Model) []Model {
	out := make([]Model, len(models))
	copy(out, models)
	sort.SliceStable(out, func(i, j int) bool {
		a, b := out[i], out[j]
		ra := ""
		if a.ReleaseDate != nil {
			ra = *a.ReleaseDate
		}
		rb := ""
		if b.ReleaseDate != nil {
			rb = *b.ReleaseDate
		}
		if ra != rb {
			if ra == "" {
				return false
			}
			if rb == "" {
				return true
			}
			return ra > rb
		}
		fa := ""
		if a.Family != nil {
			fa = *a.Family
		}
		fb := ""
		if b.Family != nil {
			fb = *b.Family
		}
		if cmp := CompareNatural(fa, fb); cmp != 0 {
			return cmp < 0
		}
		if cmp := CompareNatural(a.Name, b.Name); cmp != 0 {
			return cmp < 0
		}
		return a.ID < b.ID
	})
	return out
}
