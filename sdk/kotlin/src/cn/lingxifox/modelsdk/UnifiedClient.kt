package cn.lingxifox.modelsdk

/**
 * LMP (LingXi Model Protocol) facade over the Rust core.
 *
 * Every provider-specific payload, endpoint and response shape is produced by
 * `lingxi_core`; this class only transports bytes and hands back LMP maps.
 */
class UnifiedClient(
    @JvmField val nativeClient: NativeClient = NativeClient(),
) {
    /** Capability the protocol cannot express; thrown instead of silently degrading. */
    class UnsupportedFeatureException(
        override val message: String,
        val protocol: String,
        val feature: String,
    ) : RuntimeException(message)

    fun completeText(protocol: String, baseUrl: String, apiKey: String?, model: String, prompt: String): Map<String, Any?> =
        complete(protocol, baseUrl, apiKey, textRequest(model, prompt))

    fun complete(protocol: String, baseUrl: String, apiKey: String?, request: Map<String, Any?>): Map<String, Any?> =
        call(protocol) {
            val prepareRequest = LinkedHashMap<String, Any?>()
            prepareRequest["protocol"] = protocol
            prepareRequest["baseUrl"] = baseUrl
            if (apiKey != null) prepareRequest["apiKey"] = apiKey
            prepareRequest["request"] = request
            val target = KotlinBridge.shared().prepareUnified(prepareRequest)
            val headers = LinkedHashMap<String, String>()
            @Suppress("UNCHECKED_CAST")
            val rawHeaders = target["headers"] as? Map<String, Any?> ?: emptyMap()
            for ((key, value) in rawHeaders) headers[key] = value?.toString() ?: ""
            val url = target["url"]?.toString() ?: throw LingXiCoreException("unified_prepare returned no url")
            val body = Json.write(target["body"])
            val raw = nativeClient.postJson(url, headers, body, false)
            KotlinBridge.shared().normalizeResponse(protocol, raw)
        }

    /** Opens an incremental LMP stream. */
    fun stream(protocol: String, baseUrl: String, apiKey: String?, request: Map<String, Any?>): StreamSession {
        val openRequest = LinkedHashMap<String, Any?>()
        openRequest["protocol"] = protocol
        openRequest["baseUrl"] = baseUrl
        if (apiKey != null) openRequest["apiKey"] = apiKey
        openRequest["payload"] = request
        return StreamSession.open(openRequest, true)
    }

    /** Drains a stream into LMP events. */
    fun streamAll(protocol: String, baseUrl: String, apiKey: String?, request: Map<String, Any?>): List<Map<String, Any?>> =
        stream(protocol, baseUrl, apiKey, request).use { it.collect() }

    fun embed(protocol: String, baseUrl: String, apiKey: String?, model: String, input: List<String>): Map<String, Any?> =
        call(protocol) {
            val request = LinkedHashMap<String, Any?>()
            request["protocol"] = protocol
            request["baseUrl"] = baseUrl
            if (apiKey != null) request["apiKey"] = apiKey
            request["model"] = model
            request["input"] = input
            KotlinBridge.shared().embed(request)
        }

    fun rerank(
        protocol: String,
        baseUrl: String,
        apiKey: String?,
        model: String,
        query: String,
        documents: List<String>,
        topN: Int? = null,
    ): Map<String, Any?> = call(protocol) {
        val request = LinkedHashMap<String, Any?>()
        request["protocol"] = protocol
        request["baseUrl"] = baseUrl
        if (apiKey != null) request["apiKey"] = apiKey
        request["model"] = model
        request["query"] = query
        request["documents"] = documents
        if (topN != null) request["topN"] = topN.toLong()
        KotlinBridge.shared().rerank(request)
    }

    private inline fun <T> call(protocol: String, block: () -> T): T = try {
        block()
    } catch (e: LingXiCoreException) {
        val code = e.code ?: extractCode(e.message)
        if (code == "unsupported_feature") {
            throw UnsupportedFeatureException(e.message ?: "unsupported feature", protocol, featureName(e.message))
        }
        throw e
    }

    companion object {
        /** Minimal LMP request carrying one user text message. */
        fun textRequest(model: String, prompt: String): Map<String, Any?> {
            val content = LinkedHashMap<String, Any?>()
            content["type"] = "text"
            content["text"] = prompt
            val message = LinkedHashMap<String, Any?>()
            message["role"] = "user"
            message["content"] = listOf(content)
            val request = LinkedHashMap<String, Any?>()
            request["model"] = model
            request["messages"] = listOf(message)
            return request
        }

        private fun extractCode(message: String?): String? {
            if (message == null) return null
            val open = message.indexOf('[')
            val close = message.indexOf(']', open + 1)
            if (open < 0 || close < 0) return null
            return message.substring(open + 1, close).trim()
        }

        private fun featureName(message: String?): String {
            if (message == null) return "unknown"
            val marker = "unsupported feature '"
            val start = message.indexOf(marker)
            if (start < 0) return "unknown"
            val from = start + marker.length
            val end = message.indexOf('\'', from)
            if (end < 0) return "unknown"
            return message.substring(from, end)
        }
    }
}
