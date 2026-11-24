package cn.lingxifox.modelsdk

import java.net.URI

object ProviderRegistry {
    val ALL_PROTOCOLS = listOf(
        "openai_chat_completions", "openai_responses", "anthropic_messages", "google_gemini",
        "aws_bedrock", "google_vertex_ai", "azure_openai", "ollama", "cohere"
    )

    fun validateEndpointUrl(rawUrl: String, allowInsecureHttp: Boolean = false) {
        val uri = URI.create(rawUrl)
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
