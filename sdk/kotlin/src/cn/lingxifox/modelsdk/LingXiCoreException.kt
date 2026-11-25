package cn.lingxifox.modelsdk

/** Failure reported by the Rust core across the C ABI, with its stable code. */
class LingXiCoreException(
    message: String,
    /** Stable core error code, e.g. `unsupported_feature` or `auth_missing`. */
    val code: String? = null,
    cause: Throwable? = null,
) : RuntimeException(message, cause)
