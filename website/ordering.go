package main

import (
	"sort"
	"strings"
)

type naturalToken struct {
	isNumber bool
	raw      string
	trimmed  string
}

// tokenizeNatural splits text into ASCII digit runs and lowercased text runs,
// matching ModelCatalogOrdering in the Swift SDK.
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
		tokens = append(tokens, naturalToken{
			isNumber: true,
			raw:      raw,
			trimmed:  trimmed,
		})
		digits.Reset()
	}

	flushWords := func() {
		if words.Len() == 0 {
			return
		}
		tokens = append(tokens, naturalToken{
			isNumber: false,
			raw:      strings.ToLower(words.String()),
		})
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

// naturalCompare compares two strings using digit-aware natural ordering.
// Numbers sort before text at the same token position.
func naturalCompare(a, b string) int {
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

func sortProviders(providers []ProviderView) {
	sort.Slice(providers, func(i, j int) bool {
		a, b := providers[i], providers[j]
		if cmp := naturalCompare(a.Name, b.Name); cmp != 0 {
			return cmp < 0
		}
		return a.ID < b.ID
	})
}

func sortModels(models []ModelView) {
	sort.Slice(models, func(i, j int) bool {
		a, b := models[i], models[j]
		if a.Release != b.Release {
			if a.Release == "" {
				return false
			}
			if b.Release == "" {
				return true
			}
			return a.Release > b.Release
		}
		if cmp := naturalCompare(a.Family, b.Family); cmp != 0 {
			return cmp < 0
		}
		if cmp := naturalCompare(a.Name, b.Name); cmp != 0 {
			return cmp < 0
		}
		return a.ID < b.ID
	})
}
