package cn.lingxifox.modelsdk

import com.sun.net.httpserver.HttpServer
import java.net.InetSocketAddress
import java.nio.file.Files
import java.nio.file.Path

fun main() {
    ModelCatalog.validateSchemaV2("""{"schemaVersion":"2.0","providers":{}}""")
    val root = Path.of("../..").toAbsolutePath().normalize()
    val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
    server.createContext("/") { ex ->
        val path = ex.requestURI.path
        if (path.startsWith("/redir")) {
            ex.responseHeaders.add("Location", "http://127.0.0.1:9/leak")
            ex.sendResponseHeaders(302, -1)
            ex.close()
            return@createContext
        }
        val fixture = when {
            path.contains("converse") -> "aws_bedrock_response.json"
            path.contains("/v2/rerank") -> "cohere_rerank_response.json"
            else -> "cohere_chat_response.json"
        }
        val bytes = Files.readAllBytes(root.resolve("conformance/native/$fixture"))
        ex.responseHeaders.add("Content-Type", "application/json")
        ex.sendResponseHeaders(200, bytes.size.toLong())
        ex.responseBody.use { it.write(bytes) }
    }
    server.start()
    try {
        val base = "http://127.0.0.1:${server.address.port}"
        val client = UnifiedClient()
        val bRes = client.completeText("aws_bedrock", base, "AKIA:SECRET", "anthropic.claude-3-5-sonnet", "hi")
        check(bRes.contains("sig-bedrock-1"))
        val rr = client.rerank("cohere", base, "co-key", "rerank-v3.5", "kt", listOf("a", "b"))
        check(rr.contains("relevance_score"))
        var threw = false
        try { client.rerank("openai_chat_completions", base, "sk", "m", "q", listOf("a")) }
        catch (_: UnsupportedFeatureException) { threw = true }
        check(threw)
        var threwRedir = false
        try { client.completeText("cohere", "$base/redir", "co-key", "command-a", "hi") }
        catch (_: IllegalStateException) { threwRedir = true }
        check(threwRedir)
        println("Kotlin 4-module SDK tests passed!")
    } finally {
        server.stop(0)
    }
}
