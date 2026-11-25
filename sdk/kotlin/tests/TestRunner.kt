package cn.lingxifox.modelsdk

import cn.lingxifox.modelsdk.CatalogTypes.CatalogModel
import cn.lingxifox.modelsdk.CatalogTypes.ModelFilter
import com.sun.net.httpserver.HttpExchange
import com.sun.net.httpserver.HttpServer
import java.net.InetSocketAddress
import java.nio.charset.StandardCharsets
import java.nio.file.Files
import java.nio.file.Path
import java.time.Duration
import java.util.concurrent.atomic.AtomicInteger

/**
 * End-to-end checks for the Kotlin SDK: registry, native, unified, incremental
 * streaming, catalog conformance and catalog caching — all backed by the shared
 * Rust core and a loopback server replaying the repository conformance fixtures.
 */
object TestRunner {
    private var checks = 0

    private val root: Path = Path.of("../..").toAbsolutePath().normalize()

    @JvmStatic
    fun main(args: Array<String>) {
        registryChecks()
        val server = startProviderServer()
        try {
            val base = "http://127.0.0.1:" + server.address.port
            check(KotlinBridge.available()) {
                "Rust core is not loadable; build it with cargo build --manifest-path core/Cargo.toml"
            }
            unifiedChecks(base)
            embedAndRerankChecks(base)
            nativeChecks(base)
            streamChecks(base)
            securityChecks(base)
        } finally {
            server.stop(0)
        }
        catalogConformance()
        catalogLoader()
        println("Kotlin SDK tests passed ($checks provider checks).")
    }

    // ---- registry ---------------------------------------------------------

    private fun registryChecks() {
        val registry = ProviderRegistry.fromJson(
            Files.readString(root.resolve("registry/registry.json"), StandardCharsets.UTF_8),
        )
        check(registry.size() >= 320) { "Expected >= 320 providers, got ${registry.size()}" }
        val bedrock = registry.resolveEndpoint("amazon-bedrock", "aws_bedrock", null, false)
        check(bedrock.contains("bedrock-runtime")) { "Unexpected bedrock endpoint: $bedrock" }
        checks++
    }

    // ---- unified ----------------------------------------------------------

    private fun unifiedChecks(base: String) {
        val client = UnifiedClient()

        val bedrock = client.completeText("aws_bedrock", base, "AKID:SECRET", "anthropic.claude-3-5-sonnet", "hi")
        expect(textOf(bedrock) == "Hello from AWS Bedrock", "bedrock text was ${textOf(bedrock)}")
        expect(bedrock["finishReason"] == "tool_use", "bedrock finishReason")
        @Suppress("UNCHECKED_CAST")
        val usage = bedrock["usage"] as Map<String, Any?>
        expect((usage["inputTokens"] as Number).toInt() == 24, "bedrock input tokens")
        expect((usage["cacheReadTokens"] as Number).toInt() == 8, "bedrock cache read")
        val thinking = blocksOf(bedrock, "thinking")
        expect(thinking.size == 1, "bedrock thinking blocks")
        expect(thinking[0]["signature"] == "sig-bedrock-1", "bedrock thinking signature")
        val toolCalls = blocksOf(bedrock, "tool_call")
        expect(toolCalls.size == 1, "bedrock tool call blocks")
        expect(toolCalls[0]["name"] == "get_weather", "bedrock tool name")
        expect(!bedrock.containsKey("output"), "unified response must not leak raw provider fields")
        checks++

        val cohere = client.completeText("cohere", base, "co-key", "command-a", "hi")
        expect(textOf(cohere) == "Hello from Cohere", "cohere text")
        expect(cohere["finishReason"] == "COMPLETE", "cohere finishReason")
        checks++

        val chat = client.completeText("openai_chat_completions", base, "sk-test", "gpt-4o-mini", "hi")
        expect(textOf(chat) == "Hello from chat completions", "openai text")
        expect(chat["finishReason"] == "stop", "openai finishReason")
        checks++

        // Streaming through the core handle yields LMP events incrementally.
        val events = client.streamAll(
            "anthropic_messages", base, "sk-ant", UnifiedClient.textRequest("claude", "hi"),
        )
        val streamed = StringBuilder()
        val thinkingText = StringBuilder()
        var sawEnd = false
        var endReason: String? = null
        for (event in events) {
            when (event["type"]) {
                "text_delta" -> streamed.append(event["delta"])
                "thinking_delta" -> thinkingText.append(event["delta"])
                "message_end" -> {
                    sawEnd = true
                    endReason = event["finishReason"] as? String
                }
            }
        }
        expect(streamed.toString() == "Hello Anthropic", "streamed text was $streamed")
        expect(thinkingText.toString() == "Thinking...", "streamed thinking was $thinkingText")
        expect(sawEnd, "stream must end with message_end")
        expect(
            endReason == null || endReason == "stop" || endReason == "end_turn",
            "unexpected stream finish reason: $endReason",
        )
        checks++

        val gemini = client.completeText("google_gemini", base, "g-key", "gemini-2.0-flash", "hi")
        expect(textOf(gemini) == "Hello from Gemini", "gemini text was ${textOf(gemini)}")
        expect(gemini["finishReason"] == "STOP", "gemini finishReason")
        expect(blocksOf(gemini, "thinking").size == 1, "gemini thinking blocks")
        @Suppress("UNCHECKED_CAST")
        val geminiUsage = gemini["usage"] as Map<String, Any?>
        expect((geminiUsage["totalTokens"] as Number).toInt() == 23, "gemini total tokens")
        checks++
    }

    private fun embedAndRerankChecks(base: String) {
        val client = UnifiedClient()

        val embed = client.embed("openai_chat_completions", base, "sk-test", "text-embedding-3-small", listOf("a", "b"))
        @Suppress("UNCHECKED_CAST")
        val vectors = embed["embeddings"] as List<Any?>
        expect(vectors.size == 2, "embedding count")
        expect((vectors[0] as List<*>).size == 3, "embedding dimensions")
        checks++

        val rerank = client.rerank("cohere", base, "co-key", "rerank-v3.5", "kotlin", listOf("a", "b"))
        @Suppress("UNCHECKED_CAST")
        val results = rerank["results"] as List<Map<String, Any?>>
        expect(results.size == 2, "rerank result count")
        expect((results[0]["index"] as Number).toInt() == 1, "rerank top index")
        expect(Math.abs((results[0]["relevanceScore"] as Number).toDouble() - 0.94) < 1e-9, "rerank top score")
        checks++

        var unsupported: UnifiedClient.UnsupportedFeatureException? = null
        try {
            client.rerank("openai_chat_completions", base, "sk", "m", "q", listOf("a"))
        } catch (e: UnifiedClient.UnsupportedFeatureException) {
            unsupported = e
        }
        expect(unsupported != null, "rerank on a non-Cohere protocol must be unsupported")
        expect(unsupported!!.protocol == "openai_chat_completions", "unsupported protocol field")
        expect(unsupported.feature == "rerank", "unsupported feature name was ${unsupported.feature}")
        checks++

        var embedUnsupported = false
        try {
            client.embed("anthropic_messages", base, "sk", "m", listOf("a"))
        } catch (_: UnifiedClient.UnsupportedFeatureException) {
            embedUnsupported = true
        }
        expect(embedUnsupported, "embed on a non-OpenAI protocol must be unsupported")
        checks++
    }

    // ---- native -----------------------------------------------------------

    private fun nativeChecks(base: String) {
        val bridge = KotlinBridge.shared()
        val prepare = linkedMapOf<String, Any?>(
            "protocol" to "aws_bedrock",
            "baseUrl" to base,
            "apiKey" to "AKID:SECRET",
            "model" to "anthropic.claude-3-5-sonnet",
            "payload" to mapOf(
                "messages" to listOf(mapOf("role" to "user", "content" to listOf(mapOf("text" to "hi")))),
            ),
        )
        val target = bridge.prepareNative(prepare)
        val url = target["url"].toString()
        expect(url.endsWith("/model/anthropic.claude-3-5-sonnet/converse"), "native bedrock url $url")
        @Suppress("UNCHECKED_CAST")
        val headers = target["headers"] as Map<String, Any?>
        val authorization = headers["Authorization"].toString()
        expect(authorization.startsWith("AWS4-HMAC-SHA256"), "sigv4 header")
        expect(!authorization.contains("SECRET"), "secret must not leak")

        val flat = LinkedHashMap<String, String>()
        headers.forEach { (key, value) -> flat[key] = value.toString() }
        val raw = NativeClient().postJson(url, flat, Json.write(target["body"]), false)
        expect(raw["output"].toString().contains("Hello from AWS Bedrock"), "native raw body")
        checks++
    }

    // ---- incremental streams ---------------------------------------------

    private fun streamChecks(base: String) {
        val open = linkedMapOf<String, Any?>(
            "protocol" to "openai_chat_completions",
            "baseUrl" to base,
            "apiKey" to "sk-test",
            "model" to "gpt-4o-mini",
            "payload" to mapOf(
                "model" to "gpt-4o-mini",
                "messages" to listOf(mapOf("role" to "user", "content" to "hi")),
            ),
        )

        var frames = 0
        StreamSession.open(open, false).use { session ->
            while (true) {
                val event = session.next() ?: break
                val data = event["data"]
                expect(data is String && data.isNotEmpty(), "native frames must carry the raw provider payload")
                if (data != "[DONE]") {
                    expect(Json.parse(data as String) is Map<*, *>, "native payload must be JSON")
                    frames++
                }
            }
        }
        expect(frames == 2, "native stream should deliver both provider frames, got $frames")
        checks++

        val cancelSession = StreamSession.open(LinkedHashMap(open), false)
        expect(cancelSession.next() != null, "first event must arrive")
        cancelSession.cancel()
        var cancelled = false
        try {
            while (cancelSession.next() != null) {
                // drain until cancellation surfaces
            }
        } catch (e: LingXiCoreException) {
            cancelled = e.code?.contains("cancel") == true
        } finally {
            cancelSession.close()
        }
        expect(cancelled, "cancelling a live stream must surface a cancelled error")
        checks++

        val unifiedOpen = linkedMapOf<String, Any?>(
            "protocol" to "openai_chat_completions",
            "baseUrl" to base,
            "apiKey" to "sk-test",
            "payload" to UnifiedClient.textRequest("gpt-4o-mini", "hi"),
        )
        val text = StringBuilder()
        var lastUsage: Map<String, Any?>? = null
        StreamSession.open(unifiedOpen, true).use { session ->
            while (true) {
                val event = session.next() ?: break
                when (event["type"]) {
                    "text_delta" -> text.append(event["delta"])
                    "usage" -> {
                        @Suppress("UNCHECKED_CAST")
                        lastUsage = event["usage"] as? Map<String, Any?>
                    }
                }
            }
        }
        expect(text.toString() == "Hello world", "unified stream text was $text")
        expect(lastUsage != null, "unified stream must report usage")
        expect((lastUsage!!["totalTokens"] as Number).toInt() == 12, "unified stream usage")
        checks++
    }

    // ---- security ---------------------------------------------------------

    private fun securityChecks(base: String) {
        val client = UnifiedClient()
        var blocked = false
        try {
            client.completeText("cohere", base + "/redir", "co-key", "command-a", "hi")
        } catch (e: IllegalStateException) {
            blocked = e.message.orEmpty().contains("redirect")
        }
        expect(blocked, "redirects must be blocked")

        var insecure = false
        try {
            KotlinBridge.shared().prepareNative(
                mapOf(
                    "protocol" to "openai_chat_completions",
                    "baseUrl" to "http://example.com/v1",
                    "apiKey" to "k",
                    "payload" to mapOf<String, Any?>(),
                ),
            )
        } catch (e: LingXiCoreException) {
            insecure = e.code == "invalid_endpoint"
        }
        expect(insecure, "plain HTTP to a remote host must be rejected by the core")
        checks++
    }

    // ---- loopback fixture server -----------------------------------------

    private fun startProviderServer(): HttpServer {
        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.createContext("/") { exchange: HttpExchange ->
            val path = exchange.requestURI.path
            if (path.startsWith("/redir")) {
                exchange.responseHeaders.add("Location", "http://127.0.0.1:9/leak")
                exchange.sendResponseHeaders(302, -1)
                exchange.close()
                return@createContext
            }
            val auth = exchange.requestHeaders.getFirst("Authorization")
            if (auth != null && path.contains("converse") && !auth.startsWith("AWS4-HMAC-SHA256")) {
                throw AssertionError("Missing SigV4 header")
            }
            val streaming = isStreaming(exchange)
            var contentType = "application/json"
            val body = when {
                path.contains("converse") -> fixture("aws_bedrock_response.json")
                path.contains("/v2/rerank") -> fixture("cohere_rerank_response.json")
                path.contains("/embeddings") -> fixture("openai_embed_response.json")
                path.contains(":generateContent") -> fixture("google_gemini_response.json")
                path.contains("/messages") && streaming -> {
                    contentType = "text/event-stream"
                    fixture("anthropic_messages_stream.sse")
                }
                path.contains("/messages") -> fixture("anthropic_messages_response.json")
                path.contains("/chat/completions") && streaming -> {
                    contentType = "text/event-stream"
                    fixture("openai_chat_stream.sse")
                }
                path.contains("/chat/completions") -> fixture("openai_chat_response.json")
                else -> fixture("cohere_chat_response.json")
            }
            exchange.responseHeaders.add("Content-Type", contentType)
            exchange.sendResponseHeaders(200, body.size.toLong())
            exchange.responseBody.use { it.write(body) }
        }
        server.start()
        return server
    }

    private fun isStreaming(exchange: HttpExchange): Boolean = try {
        String(exchange.requestBody.readAllBytes(), StandardCharsets.UTF_8).contains("\"stream\":true")
    } catch (_: Exception) {
        false
    }

    private fun fixture(name: String): ByteArray =
        Files.readAllBytes(root.resolve("conformance/native").resolve(name))

    // ---- catalog conformance ---------------------------------------------

    private fun catalogConformance() {
        val expectations = Json.parse(
            Files.readString(root.resolve("conformance/catalog/expectations.json"), StandardCharsets.UTF_8),
        ) as Map<*, *>
        val catalog = LingXiModelCatalog.decode(catalogFixture("v2_catalog.json"))

        assertEq(catalog.revision.schemaVersion, expectations["schemaVersion"], "schemaVersion")
        assertEq(catalog.revision.catalogRevision, expectations["catalogRevision"], "catalogRevision")
        assertEq(catalog.revision.totalProviders.toLong(), expectations["totalProviders"], "totalProviders")
        assertEq(catalog.totalModels.toLong(), expectations["totalModels"], "totalModels")

        assertEq(catalog.sortedProviders().map { it.id }, expectations["providerOrder"], "providerOrder")
        @Suppress("UNCHECKED_CAST")
        val featured = expectations["featuredProviderOrder"] as Map<String, Any?>
        val featuredIds = (featured["featured"] as? List<*>)?.map { it.toString() } ?: emptyList()
        assertEq(
            catalog.sortedProviders(featuredIds).map { it.id },
            featured["expected"],
            "featuredProviderOrder",
        )
        assertEq(catalog.modelsForProvider("prov10").map { it.id }, expectations["prov10ModelOrder"], "prov10ModelOrder")
        assertEq(catalog.modelsForProvider("deepseek").map { it.id }, expectations["deepseekModelOrder"], "deepseekModelOrder")
        assertEq(catalog.allModels().map { it.qualifiedID }, expectations["allModelQualifiedOrder"], "allModelQualifiedOrder")

        val gpt10 = catalog.model("PROV10", "GPT-10") ?: throw AssertionError("gpt10 missing")
        assertEq(gpt10.pricing.input, 0.0, "gpt10 zero input price")
        expect(!gpt10.pricing.isUnknown, "gpt10 pricing known")
        assertEq(gpt10.capabilities.supportsTemperature, false, "gpt10 temperature=false")
        expect(gpt10.capabilities.vision, "gpt10 vision")
        expect(gpt10.capabilities.audioInput, "gpt10 audioInput")
        expect(gpt10.capabilities.audioOutput, "gpt10 audioOutput")
        assertEq(gpt10.capabilities.reasoningEfforts, listOf("low", "medium", "high"), "gpt10 efforts")
        @Suppress("UNCHECKED_CAST")
        val experimental = gpt10.fields["experimental"] as Map<String, Any?>
        assertEq(experimental["warmup"], true, "unknown experimental.warmup")

        val gpt5 = catalog.model("prov10/gpt-5") ?: throw AssertionError("gpt5 missing")
        assertEq(gpt5.contextWindow, 0L, "gpt5 context=0")
        expect(gpt5.pricing.isUnknown, "gpt5 unknown pricing")
        assertEq(gpt5.capabilities.supportsTemperature, null, "gpt5 nil temperature")
        expect(!gpt5.capabilities.structuredOutput, "gpt5 structured_output=false")

        @Suppress("UNCHECKED_CAST")
        val searchExp = expectations["search"] as Map<String, Any?>
        assertEq(catalog.search("gpt", 1).map { it.qualifiedID }, searchExp["gptLimit1"], "search gpt")
        assertEq(catalog.search("deepseek", null).map { it.qualifiedID }, searchExp["deepseek"], "search deepseek")

        @Suppress("UNCHECKED_CAST")
        val filterExp = expectations["filters"] as Map<String, Any?>
        assertEq(
            catalog.modelsMatching(ModelFilter.builder().vision(true).build()).map { it.qualifiedID },
            filterExp["vision"],
            "filter vision",
        )
        assertEq(
            catalog.modelsMatching(ModelFilter.builder().minimumContextWindow(200_000L).build()).map { it.qualifiedID },
            filterExp["minContext200k"],
            "filter 200k",
        )
        assertEq(
            catalog.modelsMatching(ModelFilter.builder().providerID("deepseek").build()).map { it.qualifiedID },
            filterExp["deepseekDefault"],
            "filter ds default",
        )
        assertEq(
            catalog.modelsMatching(
                ModelFilter.builder().providerID("deepseek").includeDeprecated(true).build(),
            ).map { it.qualifiedID },
            filterExp["deepseekIncludeDeprecated"],
            "filter ds deprecated",
        )

        val legacy = LingXiModelCatalog.decode(catalogFixture("v1_legacy.json"))
        assertEq(legacy.revision.schemaVersion, "1.0", "legacy version")
        assertEq(legacy.model("ollama", "llama3")!!.contextWindow, 8192L, "legacy context")

        val malformed = LingXiModelCatalog.decode(catalogFixture("malformed_entry.json"))
        expect(malformed.model("p", "published-key") != null, "published-key present")
        expect(malformed.model("p", "internal-id") == null, "internal-id absent")

        var unsupportedKind: CatalogException.Kind? = null
        try {
            LingXiModelCatalog.decode(catalogFixture("unsupported_schema.json"))
        } catch (e: CatalogException) {
            unsupportedKind = e.kind
        }
        assertEq(unsupportedKind, CatalogException.Kind.UNSUPPORTED_SCHEMA_VERSION, "unsupported schema kind")

        var invalidKind: CatalogException.Kind? = null
        try {
            LingXiModelCatalog.decode(catalogFixture("broken_empty_providers.json"))
        } catch (e: CatalogException) {
            invalidKind = e.kind
        }
        assertEq(invalidKind, CatalogException.Kind.INVALID_CATALOG, "empty providers kind")

        println("Kotlin catalog conformance passed.")
    }

    private fun catalogFixture(name: String): ByteArray =
        Files.readAllBytes(root.resolve("conformance/catalog/fixtures").resolve(name))

    // ---- catalog cache / ETag --------------------------------------------

    private fun catalogLoader() {
        val fixture = catalogFixture("v2_catalog.json")
        val mode = AtomicInteger(0)
        val hits = AtomicInteger(0)

        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.createContext("/models.json") { exchange: HttpExchange ->
            hits.incrementAndGet()
            if (mode.get() == 0) {
                val inm = exchange.requestHeaders.getFirst("If-None-Match")
                if ("\"rev-1\"" == inm) {
                    exchange.sendResponseHeaders(304, -1)
                } else {
                    exchange.responseHeaders.add("ETag", "\"rev-1\"")
                    exchange.sendResponseHeaders(200, fixture.size.toLong())
                    exchange.responseBody.use { it.write(fixture) }
                }
            } else {
                exchange.sendResponseHeaders(503, -1)
            }
            exchange.close()
        }
        server.start()

        val cacheDir = Files.createTempDirectory("lingxi-kotlin-cache-")
        try {
            val url = "http://127.0.0.1:" + server.address.port + "/models.json"
            val loader = CatalogLoader(
                CatalogLoader.Config(url, Duration.ofMinutes(30), cacheDir, Duration.ofSeconds(10), "Test/1.0"),
            )
            val first = loader.load(false)
            assertEq(first.revision.catalogRevision, "conf-rev-001", "loader rev")
            loader.load(false)
            assertEq(hits.get(), 1, "fresh cache skipped network")

            loader.load(true)
            assertEq(hits.get(), 2, "forceRefresh hit 304")

            mode.set(1)
            val cold = CatalogLoader(
                CatalogLoader.Config(url, Duration.ZERO, cacheDir, Duration.ofSeconds(10), "Test/1.0"),
            )
            assertEq(cold.load(false).totalModels, 5, "disk fallback models")

            val empty = CatalogLoader(
                CatalogLoader.Config(url, Duration.ZERO, null, Duration.ofSeconds(10), "Test/1.0"),
            )
            var statusKind: CatalogException.Kind? = null
            var statusCode = 0
            try {
                empty.load(false)
            } catch (e: CatalogException) {
                statusKind = e.kind
                statusCode = e.statusCode
            }
            assertEq(statusKind, CatalogException.Kind.UNEXPECTED_STATUS, "503 kind")
            assertEq(statusCode, 503, "503 status")
        } finally {
            server.stop(0)
            Files.walk(cacheDir).sorted(Comparator.reverseOrder()).forEach { Files.deleteIfExists(it) }
        }
        println("Kotlin catalog cache/ETag tests passed.")
    }

    // ---- helpers ---------------------------------------------------------

    private fun expect(condition: Boolean, message: String) {
        if (!condition) throw AssertionError(message)
    }

    private fun assertEq(actual: Any?, expected: Any?, message: String) {
        if (actual == expected) return
        if (actual is Number && expected is Number && actual.toDouble() == expected.toDouble()) return
        throw AssertionError("$message: got $actual, expected $expected")
    }

    private fun textOf(response: Map<String, Any?>): String {
        val text = StringBuilder()
        @Suppress("UNCHECKED_CAST")
        val blocks = response["content"] as? List<Any?> ?: return ""
        for (block in blocks) {
            @Suppress("UNCHECKED_CAST")
            val map = block as? Map<String, Any?> ?: continue
            if (map["type"] == "text") text.append(map["text"])
        }
        return text.toString()
    }

    private fun blocksOf(response: Map<String, Any?>, type: String): List<Map<String, Any?>> {
        val out = ArrayList<Map<String, Any?>>()
        @Suppress("UNCHECKED_CAST")
        val blocks = response["content"] as? List<Any?> ?: return out
        for (block in blocks) {
            @Suppress("UNCHECKED_CAST")
            val map = block as? Map<String, Any?> ?: continue
            if (map["type"] == type) out.add(map)
        }
        return out
    }
}
