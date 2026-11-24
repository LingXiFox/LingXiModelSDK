package cn.lingxifox.modelsdk

import java.net.URI
import java.net.http.HttpClient
import java.net.http.HttpRequest
import java.net.http.HttpResponse
import java.nio.charset.StandardCharsets
import java.security.MessageDigest
import java.time.Duration
import java.time.ZoneOffset
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

class NativeClient {
    private val http: HttpClient = HttpClient.newBuilder()
        .followRedirects(HttpClient.Redirect.NEVER)
        .connectTimeout(Duration.ofSeconds(15))
        .build()

    fun postJson(url: String, headers: Map<String, String>, jsonBody: String, allowInsecureHttp: Boolean = false): String {
        ProviderRegistry.validateEndpointUrl(url, allowInsecureHttp)
        val b = HttpRequest.newBuilder(URI.create(url))
            .timeout(Duration.ofSeconds(60))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .POST(HttpRequest.BodyPublishers.ofString(jsonBody, StandardCharsets.UTF_8))
        headers.forEach { (k, v) -> b.header(k, v) }
        val resp = http.send(b.build(), HttpResponse.BodyHandlers.ofString(StandardCharsets.UTF_8))
        if (resp.statusCode() in 300..399) {
            throw IllegalStateException("HTTP redirect ${resp.statusCode()} blocked for security")
        }
        if (resp.statusCode() !in 200..299) {
            throw IllegalStateException("Upstream HTTP ${resp.statusCode()}: ${resp.body()}")
        }
        return resp.body()
    }

    fun buildAuthHeaders(protocol: String, baseUrl: String, apiKey: String?, canonicalUri: String, payloadJson: String): Map<String, String> {
        val hdrs = linkedMapOf<String, String>()
        when (protocol) {
            "openai_chat_completions", "openai_responses", "google_vertex_ai", "cohere" -> {
                require(!apiKey.isNullOrEmpty()) { "Bearer API key required" }
                hdrs["Authorization"] = "Bearer $apiKey"
            }
            "anthropic_messages" -> {
                require(!apiKey.isNullOrEmpty()) { "x-api-key required" }
                hdrs["x-api-key"] = apiKey
                hdrs["anthropic-version"] = "2023-06-01"
            }
            "google_gemini" -> {
                require(!apiKey.isNullOrEmpty()) { "x-goog-api-key required" }
                hdrs["x-goog-api-key"] = apiKey
            }
            "azure_openai" -> {
                require(!apiKey.isNullOrEmpty()) { "api-key required" }
                hdrs["api-key"] = apiKey
            }
            "ollama" -> {}
            "aws_bedrock" -> {
                require(!apiKey.isNullOrEmpty()) { "Bedrock credentials required" }
                val parts = apiKey.split(":")
                if (parts.size >= 2) {
                    val u = URI.create(baseUrl)
                    val host = if (u.port > 0) "${u.host}:${u.port}" else u.host
                    hdrs.putAll(signSigV4("POST", canonicalUri, host, "us-east-1", "bedrock", parts[0], parts[1],
                        parts.getOrNull(2), payloadJson.toByteArray(StandardCharsets.UTF_8)))
                } else {
                    hdrs["Authorization"] = "Bearer $apiKey"
                }
            }
            else -> throw IllegalArgumentException("Unsupported protocol: $protocol")
        }
        return hdrs
    }

    companion object {
        fun signSigV4(
            method: String, canonicalUri: String, host: String, region: String, service: String,
            accessKey: String, secretKey: String, sessionToken: String?, payload: ByteArray
        ): Map<String, String> {
            val amzDate = DateTimeFormatter.ofPattern("yyyyMMdd'T'HHmmss'Z'").format(ZonedDateTime.now(ZoneOffset.UTC))
            val dateStamp = amzDate.substring(0, 8)
            val payloadHash = toHex(MessageDigest.getInstance("SHA-256").digest(payload))
            val ch = StringBuilder("host:$host\nx-amz-date:$amzDate\n")
            var sh = "host;x-amz-date"
            if (!sessionToken.isNullOrEmpty()) {
                ch.append("x-amz-security-token:$sessionToken\n")
                sh = "host;x-amz-date;x-amz-security-token"
            }
            val creq = "$method\n$canonicalUri\n\n$ch\n$sh\n$payloadHash"
            val scope = "$dateStamp/$region/$service/aws4_request"
            val sts = "AWS4-HMAC-SHA256\n$amzDate\n$scope\n${toHex(MessageDigest.getInstance("SHA-256").digest(creq.toByteArray()))}"
            fun hmac(k: ByteArray, d: String): ByteArray {
                val mac = Mac.getInstance("HmacSHA256")
                mac.init(SecretKeySpec(k, "HmacSHA256"))
                return mac.doFinal(d.toByteArray(StandardCharsets.UTF_8))
            }
            val kDate = hmac("AWS4$secretKey".toByteArray(StandardCharsets.UTF_8), dateStamp)
            val kSigning = hmac(hmac(hmac(kDate, region), service), "aws4_request")
            val sig = toHex(hmac(kSigning, sts))
            val out = linkedMapOf(
                "Authorization" to "AWS4-HMAC-SHA256 Credential=$accessKey/$scope, SignedHeaders=$sh, Signature=$sig",
                "X-Amz-Date" to amzDate
            )
            if (!sessionToken.isNullOrEmpty()) out["X-Amz-Security-Token"] = sessionToken
            return out
        }

        private fun toHex(bytes: ByteArray): String = bytes.joinToString("") { "%02x".format(it) }
    }
}
