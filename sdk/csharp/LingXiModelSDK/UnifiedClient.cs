using System.Text.Json;
using LingXiModelSDK.Native;

namespace LingXiModelSDK.Unified;

public sealed class UnsupportedFeatureException : Exception
{
    public UnsupportedFeatureException(string protocol, string feature)
        : base($"Unsupported feature '{feature}' for protocol '{protocol}'") { }
}

public sealed class UnifiedClient
{
    private readonly NativeClient _native;

    public UnifiedClient(NativeClient? native = null) => _native = native ?? new NativeClient();

    public async Task<JsonDocument> CompleteTextAsync(string protocol, string baseUrl, string? apiKey, string model, string prompt, CancellationToken ct = default)
    {
        var cleanBase = baseUrl.TrimEnd('/');
        var (uri, payloadObj) = protocol switch
        {
            "openai_chat_completions" => ("/chat/completions", (object)new { model, messages = new[] { new { role = "user", content = prompt } } }),
            "openai_responses" => ("/responses", new { model, input = new[] { new { role = "user", content = new[] { new { type = "input_text", text = prompt } } } } }),
            "anthropic_messages" => (cleanBase.EndsWith("/v1") ? "/messages" : "/v1/messages", new { model, max_tokens = 1024, messages = new[] { new { role = "user", content = new[] { new { type = "text", text = prompt } } } } }),
            "google_gemini" => ($"/v1beta/models/{model}:generateContent", new { contents = new[] { new { role = "user", parts = new[] { new { text = prompt } } } } }),
            "aws_bedrock" => ($"/model/{model}/converse", new { messages = new[] { new { role = "user", content = new[] { new { text = prompt } } } } }),
            "google_vertex_ai" => ($"/v1/publishers/google/models/{model}:generateContent", new { contents = new[] { new { role = "user", parts = new[] { new { text = prompt } } } } }),
            "azure_openai" => ($"/openai/deployments/{model}/chat/completions?api-version=2024-10-21", new { model, messages = new[] { new { role = "user", content = prompt } } }),
            "ollama" => ("/api/chat", new { model, stream = false, messages = new[] { new { role = "user", content = prompt } } }),
            "cohere" => ("/v2/chat", new { model, messages = new[] { new { role = "user", content = prompt } } }),
            _ => throw new UnsupportedFeatureException(protocol, "protocol")
        };
        var payload = JsonSerializer.Serialize(payloadObj);
        var hdrs = _native.BuildAuthHeaders(protocol, cleanBase, apiKey, uri, payload);
        return await _native.PostJsonAsync(cleanBase + uri, hdrs, payload, false, ct);
    }

    public async Task<JsonDocument> RerankAsync(string protocol, string baseUrl, string apiKey, string model, string query, IReadOnlyList<string> documents, CancellationToken ct = default)
    {
        if (protocol != "cohere") throw new UnsupportedFeatureException(protocol, "rerank");
        var payload = JsonSerializer.Serialize(new { model, query, documents });
        return await _native.PostJsonAsync(baseUrl.TrimEnd('/') + "/v2/rerank", new Dictionary<string, string> { ["Authorization"] = $"Bearer {apiKey}" }, payload, false, ct);
    }
}
