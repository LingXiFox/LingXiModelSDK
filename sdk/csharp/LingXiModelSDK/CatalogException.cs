namespace LingXiModelSDK.Catalog;

/// <summary>Why a catalog document or catalog request failed.</summary>
public enum CatalogErrorKind
{
    InvalidCatalog,
    UnsupportedSchemaVersion,
    UnexpectedStatus,
    Transport,
    Unavailable,
}

/// <summary>Catalog failure, classified so callers can tell bad data from flaky transport.</summary>
public sealed class CatalogException : Exception
{
    public CatalogException(
        CatalogErrorKind kind,
        string message,
        int statusCode = 0,
        string? schemaVersion = null,
        Exception? innerException = null)
        : base(message, innerException)
    {
        Kind = kind;
        StatusCode = statusCode;
        SchemaVersion = schemaVersion;
    }

    public CatalogErrorKind Kind { get; }

    public int StatusCode { get; }

    public string? SchemaVersion { get; }
}
