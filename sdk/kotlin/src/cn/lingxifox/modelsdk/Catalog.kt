package cn.lingxifox.modelsdk

/** Zero-inference-dependency Model Catalog wrapper for Kotlin/JVM. */
object ModelCatalog {
    fun validateSchemaV2(jsonText: String) {
        if (!jsonText.contains("\"schemaVersion\"") || !jsonText.contains("\"2.0\"")) {
            throw IllegalArgumentException("Unsupported or invalid catalog schemaVersion (expected 2.0)")
        }
    }
}
