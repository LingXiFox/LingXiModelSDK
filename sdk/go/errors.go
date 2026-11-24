package catalog

import "fmt"

// ErrorKind classifies catalog loading and decoding failures.
type ErrorKind string

const (
	ErrInvalidCatalog           ErrorKind = "invalid_catalog"
	ErrUnsupportedSchemaVersion ErrorKind = "unsupported_schema_version"
	ErrUnexpectedStatus         ErrorKind = "unexpected_status"
	ErrTransport                ErrorKind = "transport"
	ErrUnavailable              ErrorKind = "unavailable"
)

// CatalogError represents a structured failure when reading or fetching the catalog.
type CatalogError struct {
	Kind       ErrorKind
	StatusCode int
	Version    string
	Message    string
	Cause      error
}

func (e *CatalogError) Error() string {
	switch e.Kind {
	case ErrUnsupportedSchemaVersion:
		return fmt.Sprintf("unsupported catalog schema version: %s", e.Version)
	case ErrUnexpectedStatus:
		return fmt.Sprintf("unexpected HTTP status: %d", e.StatusCode)
	case ErrInvalidCatalog:
		return fmt.Sprintf("invalid catalog: %s", e.Message)
	case ErrTransport:
		return fmt.Sprintf("catalog transport error: %s", e.Message)
	default:
		return fmt.Sprintf("catalog unavailable: %s", e.Message)
	}
}

func (e *CatalogError) Unwrap() error {
	return e.Cause
}
