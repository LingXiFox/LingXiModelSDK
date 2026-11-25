package cn.lingxifox.modelsdk

import java.net.URI
import java.net.http.HttpClient
import java.net.http.HttpRequest
import java.net.http.HttpResponse
import java.nio.charset.StandardCharsets
import java.time.Duration

/**
 * Minimal HTTP transport used by the native and unified clients.
 *
 * Redirects are never followed (a redirect would leak provider credentials to
 * an attacker-chosen host) and insecure endpoints are rejected unless allowed.
 */
class NativeClient {
    private val http: HttpClient = HttpClient.newBuilder()
        .followRedirects(HttpClient.Redirect.NEVER)
        .connectTimeout(Duration.ofSeconds(15))
        .build()

    fun postJson(
        url: String,
        headers: Map<String, String>,
        jsonBody: String,
        allowInsecureHttp: Boolean = false,
    ): Map<String, Any?> {
        ProviderRegistry.validateEndpointUrl(url, allowInsecureHttp)
        val builder = HttpRequest.newBuilder(URI.create(url))
            .timeout(Duration.ofSeconds(60))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .POST(HttpRequest.BodyPublishers.ofString(jsonBody, StandardCharsets.UTF_8))
        headers.forEach { (key, value) -> builder.header(key, value) }
        val response = http.send(builder.build(), HttpResponse.BodyHandlers.ofString(StandardCharsets.UTF_8))
        val status = response.statusCode()
        if (status in 300..399) {
            throw IllegalStateException("HTTP redirect $status blocked for security")
        }
        if (status !in 200..299) {
            throw IllegalStateException("Upstream HTTP $status: ${response.body()}")
        }
        @Suppress("UNCHECKED_CAST")
        return Json.parse(response.body()) as? Map<String, Any?>
            ?: throw IllegalStateException("Upstream response is not a JSON object")
    }

    /** Resolves a provider-native payload into a wire target through the core. */
    fun prepareNative(request: Map<String, Any?>): Map<String, Any?> =
        KotlinBridge.shared().prepareNative(request)
}
