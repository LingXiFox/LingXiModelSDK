using System.Text.Json;

namespace LingXiModelSDK.Registry;

public static class Protocols
{
    public static readonly IReadOnlyList<string> All = new[]
    {
        "openai_chat_completions", "openai_responses", "anthropic_messages", "google_gemini",
        "aws_bedrock", "google_vertex_ai", "azure_openai", "ollama", "cohere"
    };
}

public sealed class ProviderRegistry
{
    private readonly Dictionary<string, JsonElement> _providers;

    private ProviderRegistry(Dictionary<string, JsonElement> providers) => _providers = providers;

    public int Count => _providers.Count;

    public static ProviderRegistry FromJson(string json)
    {
        using var doc = JsonDocument.Parse(json);
        var root = doc.RootElement;
        if (!root.TryGetProperty("schemaVersion", out var sv) || sv.GetString() != "1.0")
            throw new InvalidOperationException("Unsupported registry schemaVersion");
        var map = new Dictionary<string, JsonElement>(StringComparer.Ordinal);
        foreach (var prop in root.GetProperty("providers").EnumerateObject())
            map[prop.Name] = prop.Value.Clone();
        return new ProviderRegistry(map);
    }

    public string ResolveEndpoint(string providerId, string protocol, string? endpointOverride = null, bool allowInsecureHttp = false)
    {
        if (!_providers.TryGetValue(providerId, out var entry))
            throw new KeyNotFoundException($"Unknown provider '{providerId}'");
        var protos = entry.GetProperty("protocols").EnumerateArray().Select(x => x.GetString()).ToList();
        if (!protos.Contains(protocol))
            throw new InvalidOperationException($"Provider '{providerId}' does not declare '{protocol}'");
        string? raw = endpointOverride;
        if (string.IsNullOrWhiteSpace(raw) && entry.GetProperty("officialEndpoints").TryGetProperty(protocol, out var ep))
            raw = ep.GetString();
        if (string.IsNullOrWhiteSpace(raw))
            throw new InvalidOperationException($"No endpoint configured for {providerId}/{protocol}");
        var clean = raw.TrimEnd('/');
        ValidateEndpointUrl(clean, allowInsecureHttp);
        return clean;
    }

    public static void ValidateEndpointUrl(string rawUrl, bool allowInsecureHttp = false)
    {
        var uri = new Uri(rawUrl);
        if (uri.Scheme == "https") return;
        if (uri.Scheme == "http")
        {
            var host = uri.Host.ToLowerInvariant();
            if (allowInsecureHttp || host is "127.0.0.1" or "localhost" or "::1") return;
            throw new InvalidOperationException($"Non-HTTPS endpoint disallowed: {rawUrl}");
        }
        throw new InvalidOperationException($"Unsupported scheme: {uri.Scheme}");
    }
}
