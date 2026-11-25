package cn.lingxifox.modelsdk

import java.net.URI

/**
 * Provider registry parsed from `registry/registry.json` (schemaVersion 1.0).
 *
 * Endpoint resolution only validates URLs; all wire-protocol knowledge lives in
 * the Rust core.
 */
class ProviderRegistry private constructor(private val providers: Map<String, Any?>) {
    fun size(): Int = providers.size

    @Suppress("UNCHECKED_CAST")
    fun get(providerId: String): Map<String, Any?>? = providers[providerId] as? Map<String, Any?>

    /**
     * Resolves the base endpoint for [providerId]/[protocol], preferring
     * [endpointOverride] and falling back to the registry's official endpoint.
     */
    fun resolveEndpoint(
        providerId: String,
        protocol: String,
        endpointOverride: String?,
        allowInsecureHttp: Boolean = false,
    ): String {
        val entry = get(providerId)
            ?: throw IllegalArgumentException("Unknown provider: $providerId")
        @Suppress("UNCHECKED_CAST")
        val protocols = entry["protocols"] as? List<Any?>
        if (protocols == null || !protocols.contains(protocol)) {
            throw IllegalArgumentException("Provider '$providerId' does not declare protocol '$protocol'")
        }
        var base = endpointOverride
        if (base.isNullOrBlank()) {
            @Suppress("UNCHECKED_CAST")
            val endpoints = entry["officialEndpoints"] as? Map<String, Any?>
            base = endpoints?.get(protocol)?.toString()
        }
        if (base.isNullOrBlank()) {
            throw IllegalArgumentException("No endpoint for $providerId/$protocol")
        }
        val clean = base.replace(Regex("/+$"), "")
        validateEndpointUrl(clean, allowInsecureHttp)
        return clean
    }

    companion object {
        val ALL_PROTOCOLS: List<String> = listOf(
            "openai_chat_completions",
            "openai_responses",
            "anthropic_messages",
            "google_gemini",
            "aws_bedrock",
            "google_vertex_ai",
            "azure_openai",
            "ollama",
            "cohere",
        )

        fun fromJson(jsonText: String): ProviderRegistry {
            val parsed = Json.parse(jsonText)
            @Suppress("UNCHECKED_CAST")
            val doc = parsed as? Map<String, Any?>
                ?: throw IllegalArgumentException("Registry root must be a JSON object")
            if (doc["schemaVersion"] != "1.0") {
                throw IllegalArgumentException("Unsupported schemaVersion: ${doc["schemaVersion"]}")
            }
            @Suppress("UNCHECKED_CAST")
            val map = doc["providers"] as? Map<String, Any?>
                ?: throw IllegalArgumentException("Missing providers map")
            return ProviderRegistry(map)
        }

        fun validateEndpointUrl(rawUrl: String, allowInsecureHttp: Boolean = false) {
            val uri = try {
                URI.create(rawUrl)
            } catch (e: IllegalArgumentException) {
                throw IllegalArgumentException("Unsupported endpoint URL: $rawUrl", e)
            }
            val scheme = uri.scheme?.lowercase() ?: ""
            val host = uri.host?.lowercase() ?: ""
            if (scheme == "https" && host.isNotEmpty()) return
            if (scheme == "http" && host.isNotEmpty()) {
                if (allowInsecureHttp || host == "127.0.0.1" || host == "localhost" || host == "::1") return
                throw IllegalArgumentException("Non-HTTPS endpoint disallowed: $rawUrl")
            }
            throw IllegalArgumentException("Unsupported endpoint URL: $rawUrl")
        }
    }
}
