using System.Net.Http.Headers;
using System.Runtime.CompilerServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using LingXiModelSDK.Core;
using LingXiModelSDK.Registry;

namespace LingXiModelSDK.Native;

/// <summary>Provider-native request handed to the core for wire preparation.</summary>
public sealed record NativeRequest
{
    public required string Protocol { get; init; }

    public required string BaseUrl { get; init; }

    public string? ApiKey { get; init; }

    public string? Model { get; init; }

    /// <summary>Provider-native payload, exactly as the provider expects it.</summary>
    public JsonElement Payload { get; init; }

    public IReadOnlyDictionary<string, string>? Headers { get; init; }

    public long? TimeoutMs { get; init; }

    public long? IdleTimeoutMs { get; init; }

    public bool AllowInsecureHttp { get; init; }

    public bool Stream { get; init; }

    public static NativeRequest FromJson(string payloadJson)
    {
        using var document = JsonDocument.Parse(payloadJson);
        return new NativeRequest { Protocol = string.Empty, BaseUrl = string.Empty, Payload = document.RootElement.Clone() };
    }
}

/// <summary>Wire target resolved by the core: URL, auth headers, encoded body, framing.</summary>
public sealed record PreparedRequest(
    string Url,
    IReadOnlyDictionary<string, string> Headers,
    string Body,
    string Framing)
{
    public static PreparedRequest FromEnvelope(JsonElement envelope)
    {
        var headers = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        if (envelope.TryGetProperty("headers", out var headerElement) && headerElement.ValueKind == JsonValueKind.Object)
        {
            foreach (var entry in headerElement.EnumerateObject())
            {
                headers[entry.Name] = entry.Value.ValueKind switch
                {
                    JsonValueKind.String => entry.Value.GetString() ?? string.Empty,
                    JsonValueKind.Null => string.Empty,
                    _ => entry.Value.GetRawText(),
                };
            }
        }

        return new PreparedRequest(
            envelope.OptString("url") ?? throw new CoreException("native_prepare returned no url"),
            headers,
            envelope.TryGetProperty("body", out var body) ? body.GetRawText() : "null",
            envelope.OptString("framing") ?? "json");
    }
}

/// <summary>One raw provider stream frame: the vendor event name, its data and optional id.</summary>
public sealed record NativeStreamEvent(string? Event, string Data, string? Id)
{
    public static NativeStreamEvent FromJson(JsonElement element) => new(
        element.OptString("event"),
        element.OptString("data") ?? string.Empty,
        element.OptString("id"));

    /// <summary>Parses the frame payload as JSON; null when the frame is a sentinel such as <c>[DONE]</c>.</summary>
    public JsonDocument? ParseData()
    {
        if (string.IsNullOrWhiteSpace(Data) || Data == "[DONE]")
        {
            return null;
        }
        try
        {
            return JsonDocument.Parse(Data);
        }
        catch (JsonException)
        {
            return null;
        }
    }
}

/// <summary>
/// Provider-native entry point: the Rust core builds the request, this class only
/// posts it and reads the response back.
/// </summary>
public sealed class NativeClient : IDisposable
{
    private readonly HttpClient _http;
    private readonly CoreBridge _bridge;

    public NativeClient()
        : this(CoreBridge.Shared)
    {
    }

    public NativeClient(CoreBridge bridge)
    {
        _bridge = bridge;
        var handler = new HttpClientHandler { AllowAutoRedirect = false };
        _http = new HttpClient(handler) { Timeout = TimeSpan.FromSeconds(60) };
    }

    /// <summary>Builds a native request through <c>lingxi_native_prepare</c>.</summary>
    public PreparedRequest Prepare(NativeRequest request)
    {
        var envelope = _bridge.NativePrepare(BuildPrepareJson(request, unified: false));
        return PreparedRequest.FromEnvelope(envelope);
    }

    private static string BuildPrepareJson(NativeRequest request, bool unified)
    {
        var json = CoreRequests.Base(request.Protocol, request.BaseUrl, request.ApiKey);
        if (!string.IsNullOrEmpty(request.Model))
        {
            json["model"] = request.Model;
        }
        if (request.Payload.ValueKind is not (JsonValueKind.Undefined or JsonValueKind.Null))
        {
            json["payload"] = CoreRequests.Payload(request.Payload);
        }
        CoreRequests.Headers(json, request.Headers);
        CoreRequests.Limits(json, request.TimeoutMs, request.IdleTimeoutMs, request.AllowInsecureHttp, request.Stream);
        return json.ToJsonString();
    }

    /// <summary>POSTs an already-prepared body and parses the JSON response.</summary>
    public async Task<JsonDocument> PostJsonAsync(
        string url,
        IReadOnlyDictionary<string, string> headers,
        string jsonBody,
        bool allowInsecureHttp = false,
        CancellationToken ct = default)
    {
        ProviderRegistry.ValidateEndpointUrl(url, allowInsecureHttp);
        using var req = new HttpRequestMessage(HttpMethod.Post, url)
        {
            Content = new StringContent(jsonBody, Encoding.UTF8, "application/json"),
        };
        req.Headers.Accept.Add(new MediaTypeWithQualityHeaderValue("application/json"));
        foreach (var kv in headers)
        {
            req.Headers.TryAddWithoutValidation(kv.Key, kv.Value);
        }

        using var resp = await _http.SendAsync(req, ct).ConfigureAwait(false);
        var code = (int)resp.StatusCode;
        var body = await resp.Content.ReadAsStringAsync(ct).ConfigureAwait(false);
        if (code is >= 300 and <= 399)
        {
            throw new InvalidOperationException($"HTTP redirect {code} blocked for security");
        }
        if (!resp.IsSuccessStatusCode)
        {
            throw new HttpRequestException($"Upstream HTTP {code}: {body}");
        }
        return JsonDocument.Parse(body);
    }

    /// <summary>
    /// Streams raw provider frames incrementally; each yielded frame corresponds to
    /// one pull from the core, so the upstream is not read ahead.
    /// </summary>
    public async IAsyncEnumerable<NativeStreamEvent> StreamRawAsync(
        NativeRequest request,
        ulong timeoutMs = StreamSession.DefaultTimeoutMs,
        [EnumeratorCancellation] CancellationToken ct = default)
    {
        var json = CoreRequests.Base(request.Protocol, request.BaseUrl, request.ApiKey);
        if (!string.IsNullOrEmpty(request.Model))
        {
            json["model"] = request.Model;
        }
        json["payload"] = CoreRequests.Payload(request.Payload);
        CoreRequests.Headers(json, request.Headers);
        CoreRequests.Limits(json, request.TimeoutMs, request.IdleTimeoutMs, request.AllowInsecureHttp, stream: null);
        json["unified"] = false;

        var session = StreamSession.Open(_bridge, json.ToJsonString());
        try
        {
            using var registration = ct.Register(session.Cancel);
            while (true)
            {
                JsonElement? frame;
                try
                {
                    frame = await Task.Run(() => session.Next(timeoutMs)).ConfigureAwait(false);
                }
                catch (CoreException error) when (ct.IsCancellationRequested
                    && (error.Code?.Contains("cancel", StringComparison.OrdinalIgnoreCase) ?? false))
                {
                    throw new OperationCanceledException("stream cancelled", error, ct);
                }

                if (frame is null)
                {
                    yield break;
                }
                yield return NativeStreamEvent.FromJson(frame.Value);
            }
        }
        finally
        {
            session.Dispose();
        }
    }

    public void Dispose() => _http.Dispose();
}
