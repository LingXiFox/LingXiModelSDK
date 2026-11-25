package cn.lingxifox.modelsdk

/** Catalog failures, classified so callers can distinguish bad data from transport flakiness. */
class CatalogException(
    val kind: Kind,
    message: String,
    val statusCode: Int = 0,
    val schemaVersion: String? = null,
    cause: Throwable? = null,
) : RuntimeException(message, cause) {
    enum class Kind {
        INVALID_CATALOG,
        UNSUPPORTED_SCHEMA_VERSION,
        UNEXPECTED_STATUS,
        TRANSPORT,
        UNAVAILABLE,
    }
}
