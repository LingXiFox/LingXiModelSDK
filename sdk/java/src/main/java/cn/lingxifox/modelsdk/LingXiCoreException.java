package cn.lingxifox.modelsdk;

/** Failure reported by the Rust core across the C ABI, with its stable code. */
public class LingXiCoreException extends RuntimeException {
    private final String code;

    public LingXiCoreException(String message) {
        this(message, null);
    }

    public LingXiCoreException(String message, Throwable cause) {
        super(message, cause);
        this.code = null;
    }

    public LingXiCoreException(String message, String code, Throwable cause) {
        super(message, cause);
        this.code = code;
    }

    /** Stable core error code, e.g. {@code unsupported_feature} or {@code auth_missing}. */
    public String code() {
        return code;
    }
}
