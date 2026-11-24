package cn.lingxifox.modelsdk;

public class CatalogException extends RuntimeException {
    public enum Kind {
        INVALID_CATALOG,
        UNSUPPORTED_SCHEMA_VERSION,
        UNEXPECTED_STATUS,
        TRANSPORT,
        UNAVAILABLE
    }

    private final Kind kind;
    private final int statusCode;
    private final String schemaVersion;

    public CatalogException(Kind kind, String message) {
        this(kind, message, 0, null, null);
    }

    public CatalogException(Kind kind, String message, int statusCode, String schemaVersion, Throwable cause) {
        super(message, cause);
        this.kind = kind;
        this.statusCode = statusCode;
        this.schemaVersion = schemaVersion;
    }

    public Kind getKind() {
        return kind;
    }

    public int getStatusCode() {
        return statusCode;
    }

    public String getSchemaVersion() {
        return schemaVersion;
    }
}
