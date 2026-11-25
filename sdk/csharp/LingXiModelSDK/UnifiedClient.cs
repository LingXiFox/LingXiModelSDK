using System.Text.Json;
using System.Text.Json.Nodes;
using LingXiModelSDK.Core;
using LingXiModelSDK.Lmp;
using LingXiModelSDK.Native;

namespace LingXiModelSDK.Unified;

/// <summary>Thrown when a protocol cannot express a requested LMP feature.</summary>
public sealed class UnsupportedFeatureException : Exception
{
    public UnsupportedFeatureException(string message, string protocol, string feature)
        : base(message)
    {
        Protocol = protocol;
        Feature = feature;
    }

    public string Protocol { get; }

    public string Feature { get; }
}

/// <summary>
/// Opt-in normalized client: LMP requests go to the core for translation, the
/// native call is made here, and the core normalizes the response back to LMP.
/// </summary>
public sealed class UnifiedClient : IDisposable
{
    private readonly NativeClient _native;
    private readonly CoreBridge _bridge;

    public UnifiedClient()
        : this(new NativeClient())
    {
    }

    public UnifiedClient(NativeClient native)
    {
        _native = native;
        _bridge = CoreBridge.Shared;
    }

    public static bool Available => CoreBridge.Available;

    public static string? LoadFailure => CoreBridge.LoadFailure;

    public static LmpRequest TextRequest(string model, string prompt) => new()
    {
        Model = model,
        Messages = new[] { LmpMessage.User(prompt) },
    };

    /// <summary>Sends an LMP request to a provider and returns the normalized response.</summary>
    public async Task<LmpResponse> CompleteAsync(
        string protocol,
        string baseUrl,
        string? apiKey,
        LmpRequest request,
        CancellationToken ct = default)
    {
        var json = CoreRequests.Base(protocol, baseUrl, apiKey);
        json["request"] = CoreRequests.Payload(request);
        JsonElement raw;
        try
        {
            var prepared = PreparedRequest.FromEnvelope(_bridge.UnifiedPrepare(json.ToJsonString()));
            using var document = await _native
                .PostJsonAsync(prepared.Url, prepared.Headers, prepared.Body, false, ct)
                .ConfigureAwait(false);
            raw = document.RootElement.Clone();
        }
        catch (CoreException error)
        {
            throw Map(error, protocol);
        }

        try
        {
            return LmpResponse.FromJson(_bridge.NormalizeResponse(protocol, raw.GetRawText()));
        }
        catch (CoreException error)
        {
            throw Map(error, protocol);
        }
    }

    /// <summary>Convenience wrapper for a single user prompt.</summary>
    public Task<LmpResponse> CompleteTextAsync(
        string protocol,
        string baseUrl,
        string? apiKey,
        string model,
        string prompt,
        CancellationToken ct = default)
        => CompleteAsync(protocol, baseUrl, apiKey, TextRequest(model, prompt), ct);

    /// <summary>
    /// Streams LMP events incrementally: each event arrives as soon as the upstream
    /// produces it, without waiting for the full response.
    /// </summary>
    public async IAsyncEnumerable<LmpStreamEvent> StreamAsync(
        string protocol,
        string baseUrl,
        string? apiKey,
        LmpRequest request,
        ulong timeoutMs = StreamSession.DefaultTimeoutMs,
        [System.Runtime.CompilerServices.EnumeratorCancellation] CancellationToken ct = default)
    {
        var json = CoreRequests.Base(protocol, baseUrl, apiKey);
        json["payload"] = CoreRequests.Payload(request);
        json["unified"] = true;

        StreamSession session;
        try
        {
            session = StreamSession.Open(_bridge, json.ToJsonString());
        }
        catch (CoreException error)
        {
            throw Map(error, protocol);
        }

        try
        {
            using var registration = ct.Register(session.Cancel);
            while (true)
            {
                JsonElement? frame;
                try
                {
                    frame = await Task.Run(() => session.Next(timeoutMs), ct).ConfigureAwait(false);
                }
                catch (CoreException error) when (ct.IsCancellationRequested
                    && (error.Code?.Contains("cancel", StringComparison.OrdinalIgnoreCase) ?? false))
                {
                    throw new OperationCanceledException("stream cancelled", error, ct);
                }
                catch (CoreException error)
                {
                    throw Map(error, protocol);
                }

                if (frame is null)
                {
                    yield break;
                }
                yield return LmpStreamEvent.FromJson(frame.Value);
            }
        }
        finally
        {
            session.Dispose();
        }
    }

    /// <summary>Collects a whole stream; useful for tests and non-incremental callers.</summary>
    public async Task<IReadOnlyList<LmpStreamEvent>> StreamAllAsync(
        string protocol,
        string baseUrl,
        string? apiKey,
        LmpRequest request,
        ulong timeoutMs = StreamSession.DefaultTimeoutMs,
        CancellationToken ct = default)
    {
        var events = new List<LmpStreamEvent>();
        await foreach (var @event in StreamAsync(protocol, baseUrl, apiKey, request, timeoutMs, ct).ConfigureAwait(false))
        {
            events.Add(@event);
        }
        return events;
    }

    public async Task<LmpEmbeddingResponse> EmbedAsync(
        string protocol,
        string baseUrl,
        string? apiKey,
        string model,
        IReadOnlyList<string> input,
        CancellationToken ct = default)
    {
        var json = CoreRequests.Base(protocol, baseUrl, apiKey);
        json["model"] = model;
        var array = new JsonArray();
        foreach (var item in input)
        {
            array.Add(item);
        }
        json["input"] = array;

        try
        {
            return LmpEmbeddingResponse.FromJson(
                await Task.Run(() => _bridge.Embed(json.ToJsonString()), ct).ConfigureAwait(false));
        }
        catch (CoreException error)
        {
            throw Map(error, protocol);
        }
    }

    public async Task<LmpRerankResponse> RerankAsync(
        string protocol,
        string baseUrl,
        string? apiKey,
        string model,
        string query,
        IReadOnlyList<string> documents,
        int? topN = null,
        CancellationToken ct = default)
    {
        var json = CoreRequests.Base(protocol, baseUrl, apiKey);
        json["model"] = model;
        json["query"] = query;
        var array = new JsonArray();
        foreach (var document in documents)
        {
            array.Add(document);
        }
        json["documents"] = array;
        if (topN is { } top)
        {
            json["topN"] = top;
        }

        try
        {
            return LmpRerankResponse.FromJson(
                await Task.Run(() => _bridge.Rerank(json.ToJsonString()), ct).ConfigureAwait(false));
        }
        catch (CoreException error)
        {
            throw Map(error, protocol);
        }
    }

    /// <summary>Translates an LMP request into the provider-native payload envelope.</summary>
    public JsonElement TranslateRequest(string protocol, LmpRequest request)
        => _bridge.TranslateRequest(protocol, request.ToJson());

    /// <summary>Normalizes a native response body (or a JSON array of stream frames) into LMP.</summary>
    public JsonElement NormalizeResponse(string protocol, string nativeResponseJson)
        => _bridge.NormalizeResponse(protocol, nativeResponseJson);

    public IReadOnlyList<JsonElement> NormalizeStreamEvents(string protocol, string eventsJson)
        => _bridge.NormalizeStreamEvents(protocol, eventsJson);

    private static Exception Map(CoreException error, string protocol)
    {
        var code = error.Code ?? ExtractCode(error.Message);
        if (code == "unsupported_feature")
        {
            return new UnsupportedFeatureException(error.Message, protocol, FeatureName(error.Message));
        }
        return error;
    }

    private static string? ExtractCode(string message)
    {
        var start = message.IndexOf('[');
        var end = message.IndexOf(']');
        return start >= 0 && end > start ? message[(start + 1)..end] : null;
    }

    private static string FeatureName(string message)
    {
        const string marker = "unsupported feature '";
        var start = message.IndexOf(marker, StringComparison.Ordinal);
        if (start < 0)
        {
            return "unknown";
        }
        start += marker.Length;
        var end = message.IndexOf('\'', start);
        return end > start ? message[start..end] : "unknown";
    }

    public void Dispose() => _native.Dispose();
}
