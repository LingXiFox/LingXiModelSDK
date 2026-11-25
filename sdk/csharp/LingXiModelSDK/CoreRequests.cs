using System.Text.Json;
using System.Text.Json.Nodes;
using LingXiModelSDK.Lmp;

namespace LingXiModelSDK.Core;

/// <summary>Builders for the camelCase request envelopes the C ABI accepts.</summary>
internal static class CoreRequests
{
    public static JsonObject Base(string protocol, string baseUrl, string? apiKey)
    {
        var request = new JsonObject
        {
            ["protocol"] = protocol,
            ["baseUrl"] = baseUrl,
        };
        if (!string.IsNullOrEmpty(apiKey))
        {
            request["apiKey"] = apiKey;
        }
        return request;
    }

    public static JsonNode Payload(LmpRequest request) => JsonNode.Parse(request.ToJson())!;

    public static JsonNode Payload(JsonElement payload) => JsonNode.Parse(payload.GetRawText())!;

    public static void Headers(JsonObject request, IReadOnlyDictionary<string, string>? headers)
    {
        if (headers is null || headers.Count == 0)
        {
            return;
        }
        var map = new JsonObject();
        foreach (var (name, value) in headers)
        {
            map[name] = value;
        }
        request["headers"] = map;
    }

    public static void Limits(JsonObject request, long? timeoutMs, long? idleTimeoutMs, bool allowInsecureHttp, bool? stream)
    {
        if (timeoutMs is { } timeout)
        {
            request["timeoutMs"] = timeout;
        }
        if (idleTimeoutMs is { } idle)
        {
            request["idleTimeoutMs"] = idle;
        }
        if (allowInsecureHttp)
        {
            request["allowInsecureHttp"] = true;
        }
        if (stream is { } streaming)
        {
            request["stream"] = streaming;
        }
    }
}
