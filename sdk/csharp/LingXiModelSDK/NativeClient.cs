using System.Net.Http.Headers;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using LingXiModelSDK.Registry;

namespace LingXiModelSDK.Native;

public sealed class NativeClient : IDisposable
{
    private readonly HttpClient _http;

    public NativeClient()
    {
        var handler = new HttpClientHandler { AllowAutoRedirect = false };
        _http = new HttpClient(handler) { Timeout = TimeSpan.FromSeconds(60) };
    }

    public async Task<JsonDocument> PostJsonAsync(string url, Dictionary<string, string> headers, string jsonBody, bool allowInsecureHttp = false, CancellationToken ct = default)
    {
        ProviderRegistry.ValidateEndpointUrl(url, allowInsecureHttp);
        using var req = new HttpRequestMessage(HttpMethod.Post, url)
        {
            Content = new StringContent(jsonBody, Encoding.UTF8, "application/json")
        };
        req.Headers.Accept.Add(new MediaTypeWithQualityHeaderValue("application/json"));
        foreach (var kv in headers) req.Headers.TryAddWithoutValidation(kv.Key, kv.Value);

        using var resp = await _http.SendAsync(req, ct);
        var code = (int)resp.StatusCode;
        var body = await resp.Content.ReadAsStringAsync(ct);
        if (code is >= 300 and <= 399)
            throw new InvalidOperationException($"HTTP redirect {code} blocked for security");
        if (!resp.IsSuccessStatusCode)
            throw new HttpRequestException($"Upstream HTTP {code}: {body}");
        return JsonDocument.Parse(body);
    }

    public Dictionary<string, string> BuildAuthHeaders(string protocol, string baseUrl, string? apiKey, string canonicalUri, string payloadJson)
    {
        var hdrs = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        switch (protocol)
        {
            case "openai_chat_completions":
            case "openai_responses":
            case "google_vertex_ai":
            case "cohere":
                hdrs["Authorization"] = $"Bearer {RequireKey(apiKey)}";
                break;
            case "anthropic_messages":
                hdrs["x-api-key"] = RequireKey(apiKey);
                hdrs["anthropic-version"] = "2023-06-01";
                break;
            case "google_gemini":
                hdrs["x-goog-api-key"] = RequireKey(apiKey);
                break;
            case "azure_openai":
                hdrs["api-key"] = RequireKey(apiKey);
                break;
            case "ollama":
                break;
            case "aws_bedrock":
                var key = RequireKey(apiKey);
                var parts = key.Split(':');
                if (parts.Length >= 2)
                {
                    var u = new Uri(baseUrl);
                    var host = u.IsDefaultPort ? u.Host : $"{u.Host}:{u.Port}";
                    foreach (var kv in SignSigV4("POST", canonicalUri, host, "us-east-1", "bedrock", parts[0], parts[1], parts.Length >= 3 ? parts[2] : null, Encoding.UTF8.GetBytes(payloadJson)))
                        hdrs[kv.Key] = kv.Value;
                }
                else
                {
                    hdrs["Authorization"] = $"Bearer {key}";
                }
                break;
            default:
                throw new NotSupportedException($"Unsupported protocol: {protocol}");
        }
        return hdrs;
    }

    public static Dictionary<string, string> SignSigV4(string method, string canonicalUri, string host, string region, string service, string accessKey, string secretKey, string? sessionToken, byte[] payload)
    {
        var amzDate = DateTime.UtcNow.ToString("yyyyMMddTHHmmssZ");
        var dateStamp = amzDate[..8];
        var payloadHash = Convert.ToHexString(SHA256.HashData(payload)).ToLowerInvariant();
        var canonicalHeaders = $"host:{host}\nx-amz-date:{amzDate}\n" + (string.IsNullOrEmpty(sessionToken) ? "" : $"x-amz-security-token:{sessionToken}\n");
        var signedHeaders = string.IsNullOrEmpty(sessionToken) ? "host;x-amz-date" : "host;x-amz-date;x-amz-security-token";
        var canonicalReq = $"{method}\n{canonicalUri}\n\n{canonicalHeaders}\n{signedHeaders}\n{payloadHash}";
        var scope = $"{dateStamp}/{region}/{service}/aws4_request";
        var sts = $"AWS4-HMAC-SHA256\n{amzDate}\n{scope}\n{Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(canonicalReq))).ToLowerInvariant()}";

        static byte[] Hmac(byte[] k, string d) => HMACSHA256.HashData(k, Encoding.UTF8.GetBytes(d));
        var kDate = Hmac(Encoding.UTF8.GetBytes("AWS4" + secretKey), dateStamp);
        var kRegion = Hmac(kDate, region);
        var kService = Hmac(kRegion, service);
        var kSigning = Hmac(kService, "aws4_request");
        var sig = Convert.ToHexString(Hmac(kSigning, sts)).ToLowerInvariant();

        var outMap = new Dictionary<string, string>
        {
            ["Authorization"] = $"AWS4-HMAC-SHA256 Credential={accessKey}/{scope}, SignedHeaders={signedHeaders}, Signature={sig}",
            ["X-Amz-Date"] = amzDate
        };
        if (!string.IsNullOrEmpty(sessionToken)) outMap["X-Amz-Security-Token"] = sessionToken;
        return outMap;
    }

    private static string RequireKey(string? k) => string.IsNullOrWhiteSpace(k) ? throw new InvalidOperationException("API key is required") : k;

    public void Dispose() => _http.Dispose();
}
