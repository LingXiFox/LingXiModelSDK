"""Structured errors for LingXiModelSDK."""


class ModelCatalogError(Exception):
    """Base error raised when loading or decoding a model catalog fails."""


class InvalidCatalogError(ModelCatalogError):
    """Raised when the catalog JSON structure is invalid."""


class UnsupportedSchemaVersionError(ModelCatalogError):
    """Raised when the catalog schema major version is unsupported."""

    def __init__(self, version: str) -> None:
        self.version = version
        super().__init__(f"Unsupported catalog schema version: {version}")


class UnexpectedStatusError(ModelCatalogError):
    """Raised when the catalog endpoint returns a non-2xx HTTP status."""

    def __init__(self, status_code: int) -> None:
        self.status_code = status_code
        super().__init__(f"Unexpected HTTP status: {status_code}")


class TransportError(ModelCatalogError):
    """Raised when a network transport error occurs."""


class UnavailableError(ModelCatalogError):
    """Raised when no fresh or cached catalog is available."""
