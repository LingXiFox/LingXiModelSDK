package cn.lingxifox.modelsdk

class UnsupportedFeatureException(protocol: String, feature: String) :
    RuntimeException("Unsupported feature '$feature' for protocol '$protocol'")

class UnifiedClient(private val nativeClient: NativeClient = NativeClient()) {
    fun completeText(protocol: String, baseUrl: String, apiKey: String?, model: String, prompt: String): String {
        val cleanBase = baseUrl.trimEnd('/')
        val escPrompt = prompt.replace("\"", "\\\"")
        val (uri, payload) = when (protocol) {
            "openai_chat_completions" -> "/chat/completions" to """{"model":"$model","messages":[{"role":"user","content":"$escPrompt"}]}"""
            "openai_responses" -> "/responses" to """{"model":"$model","input":[{"role":"user","content":[{"type":"input_text","text":"$escPrompt"}]}]}"""
            "anthropic_messages" -> (if (cleanBase.endsWith("/v1")) "/messages" else "/v1/messages") to """{"model":"$model","max_tokens":1024,"messages":[{"role":"user","content":[{"type":"text","text":"$escPrompt"}]}]}"""
            "google_gemini" -> "/v1beta/models/$model:generateContent" to """{"contents":[{"role":"user","parts":[{"text":"$escPrompt"}]}]}"""
            "aws_bedrock" -> "/model/$model/converse" to """{"messages":[{"role":"user","content":[{"text":"$escPrompt"}]}]}"""
            "google_vertex_ai" -> "/v1/publishers/google/models/$model:generateContent" to """{"contents":[{"role":"user","parts":[{"text":"$escPrompt"}]}]}"""
            "azure_openai" -> "/openai/deployments/$model/chat/completions?api-version=2024-10-21" to """{"model":"$model","messages":[{"role":"user","content":"$escPrompt"}]}"""
            "ollama" -> "/api/chat" to """{"model":"$model","stream":false,"messages":[{"role":"user","content":"$escPrompt"}]}"""
            "cohere" -> "/v2/chat" to """{"model":"$model","messages":[{"role":"user","content":"$escPrompt"}]}"""
            else -> throw UnsupportedFeatureException(protocol, "protocol")
        }
        val hdrs = nativeClient.buildAuthHeaders(protocol, cleanBase, apiKey, uri, payload)
        return nativeClient.postJson(cleanBase + uri, hdrs, payload)
    }

    fun rerank(protocol: String, baseUrl: String, apiKey: String, model: String, query: String, documents: List<String>): String {
        if (protocol != "cohere") throw UnsupportedFeatureException(protocol, "rerank")
        val docs = documents.joinToString(",") { "\"${it.replace("\"", "\\\"")}\"" }
        val payload = """{"model":"$model","query":"$query","documents":[$docs]}"""
        return nativeClient.postJson(baseUrl.trimEnd('/') + "/v2/rerank", mapOf("Authorization" to "Bearer $apiKey"), payload)
    }
}
