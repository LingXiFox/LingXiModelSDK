using System.Diagnostics;
using LingXiModelSDK;
using System.Text.Json;
using LingXiModelSDK.Catalog;
using LingXiModelSDK.Core;
using LingXiModelSDK.Lmp;
using LingXiModelSDK.Native;
using LingXiModelSDK.Registry;
using LingXiModelSDK.Unified;
using LingXiModelSdkTests;

static string FindRepoRoot()
{
    foreach (var start in new[] { Directory.GetCurrentDirectory(), AppContext.BaseDirectory })
    {
        var dir = new DirectoryInfo(start);
        while (dir != null)
        {
            if (File.Exists(Path.Combine(dir.FullName, "registry", "registry.json")))
            {
                return dir.FullName;
            }
            dir = dir.Parent;
        }
    }
    throw new DirectoryNotFoundException("Could not locate repository root containing registry/registry.json");
}

static void Check(bool condition, string message)
{
    if (!condition)
    {
        throw new Exception(message);
    }
}

static async Task<T> Within<T>(Task<T> task, int seconds, string what)
    => await task.WaitAsync(TimeSpan.FromSeconds(seconds));

var root = FindRepoRoot();

// ---- Registry (unchanged surface) -----------------------------------------
var registry = ProviderRegistry.FromJson(await File.ReadAllTextAsync(Path.Combine(root, "registry", "registry.json")));
Check(registry.Count >= 320, $"Expected >=320 providers, got {registry.Count}");
Check(
    registry.ResolveEndpoint("amazon-bedrock", "aws_bedrock").Contains("bedrock-runtime", StringComparison.Ordinal),
    "Bedrock endpoint should target bedrock-runtime");

// ---- Core bridge ----------------------------------------------------------
Check(CoreBridge.Available, "Rust core unavailable: " + CoreBridge.LoadFailure);
Console.WriteLine($"core library: {CoreBridge.Shared.LibraryPath}");

using var server = new LoopbackServer(root);
var baseUrl = server.BaseUrl;

// ---- Unified completion: LMP shapes --------------------------------------
using (var unified = new UnifiedClient())
{
    var bedrock = await Within(
        unified.CompleteTextAsync("aws_bedrock", baseUrl, "AKID:SECRET", "anthropic.claude-3-5-sonnet", "hi"),
        30,
        "bedrock completion");
    Check(bedrock.Text == "Hello from AWS Bedrock", $"bedrock text: {bedrock.Text}");
    Check(bedrock.FinishReason == "tool_use", $"bedrock finishReason: {bedrock.FinishReason}");
    Check(bedrock.Usage.InputTokens == 24, $"bedrock inputTokens: {bedrock.Usage.InputTokens}");
    Check(bedrock.Usage.CacheReadTokens == 8, $"bedrock cacheReadTokens: {bedrock.Usage.CacheReadTokens}");
    var bedrockThinking = bedrock.Content.OfType<LmpThinkingBlock>().ToArray();
    Check(bedrockThinking.Length == 1 && bedrockThinking[0].Signature == "sig-bedrock-1",
        $"bedrock thinking blocks: {bedrockThinking.Length}");
    var bedrockCalls = bedrock.Content.OfType<LmpToolCallBlock>().ToArray();
    Check(bedrockCalls.Length == 1 && bedrockCalls[0].Name == "get_weather",
        $"bedrock tool calls: {bedrockCalls.Length}");
    Check(!bedrock.Raw.TryGetProperty("output", out _), "bedrock raw should be the LMP envelope");

    var cohere = await Within(unified.CompleteTextAsync("cohere", baseUrl, "co-key", "command-a", "hi"), 30, "cohere completion");
    Check(cohere.Text == "Hello from Cohere", $"cohere text: {cohere.Text}");
    Check(cohere.FinishReason == "COMPLETE", $"cohere finishReason: {cohere.FinishReason}");

    var openai = await Within(unified.CompleteTextAsync("openai_chat_completions", baseUrl, "sk-test", "gpt-4o-mini", "hi"), 30, "openai completion");
    Check(openai.Text == "Hello from chat completions", $"openai text: {openai.Text}");
    Check(openai.FinishReason == "stop", $"openai finishReason: {openai.FinishReason}");

    var gemini = await Within(unified.CompleteTextAsync("google_gemini", baseUrl, "gm-key", "gemini-2.5-pro", "hi"), 30, "gemini completion");
    Check(gemini.Text == "Hello from Gemini", $"gemini text: {gemini.Text}");
    Check(gemini.FinishReason == "STOP", $"gemini finishReason: {gemini.FinishReason}");
    Check(gemini.Content.OfType<LmpThinkingBlock>().Count() == 1, "gemini should expose one thinking block");
    Check(gemini.Usage.TotalTokens == 23, $"gemini totalTokens: {gemini.Usage.TotalTokens}");

    // ---- Incremental streaming --------------------------------------------
    var streamRequest = UnifiedClient.TextRequest("gpt-4o-mini", "hi");
    var events = new List<LmpStreamEvent>();
    var firstEventWhileServerWriting = false;
    var stopwatch = Stopwatch.StartNew();
    using var streamTimeout = new CancellationTokenSource(TimeSpan.FromSeconds(30));
    await foreach (var @event in unified.StreamAsync("openai_chat_completions", baseUrl + "/incremental", "sk-test", streamRequest, ct: streamTimeout.Token))
    {
        if (events.Count == 0)
        {
            firstEventWhileServerWriting = !server.FirstFrameConsumed.Task.IsCompleted;
            server.FirstFrameConsumed.TrySetResult();
        }
        events.Add(@event);
    }
    stopwatch.Stop();
    Check(server.IncrementalHits == 1, $"incremental stream hits: {server.IncrementalHits}");
    Check(firstEventWhileServerWriting,
        "the first stream event must arrive while the server is still writing the response");
    Check(events.Count > 0 && events[0].Kind == LmpStreamEventKind.TextDelta,
        $"first stream event: {(events.Count == 0 ? "none" : events[0].Type)}");
    var streamedText = string.Concat(events.Where(e => e.Kind == LmpStreamEventKind.TextDelta).Select(e => e.Delta));
    Check(streamedText == "Hello world", $"streamed text: {streamedText}");
    var usageEvent = events.LastOrDefault(e => e.Kind == LmpStreamEventKind.Usage);
    Check(usageEvent?.Usage?.TotalTokens == 12, $"streamed usage: {usageEvent?.Usage?.TotalTokens}");
    Check(events.Any(e => e.Kind == LmpStreamEventKind.MessageEnd), "stream should end with message_end");

    // ---- Anthropic streaming ----------------------------------------------
    var anthropicEvents = await Within(
        unified.StreamAllAsync("anthropic_messages", baseUrl, "ak-test", UnifiedClient.TextRequest("claude-sonnet-4", "hi")),
        30,
        "anthropic stream");
    var anthropicText = string.Concat(anthropicEvents.Where(e => e.Kind == LmpStreamEventKind.TextDelta).Select(e => e.Delta));
    var anthropicThinking = string.Concat(anthropicEvents.Where(e => e.Kind == LmpStreamEventKind.ThinkingDelta).Select(e => e.Delta));
    Check(anthropicText == "Hello Anthropic", $"anthropic stream text: {anthropicText}");
    Check(anthropicThinking == "Thinking...", $"anthropic stream thinking: {anthropicThinking}");
    Check(anthropicEvents.Any(e => e.Kind == LmpStreamEventKind.MessageEnd), "anthropic stream should end");

    // ---- Embeddings and reranking -----------------------------------------
    var embedding = await Within(
        unified.EmbedAsync("openai_chat_completions", baseUrl, "sk-test", "text-embedding-3-small", new[] { "a", "b" }),
        30,
        "openai embed");
    Check(embedding.Embeddings.Count == 2, $"embedding vectors: {embedding.Embeddings.Count}");
    Check(embedding.Embeddings.All(v => v.Count == 3), "each embedding should have 3 dimensions");

    var rerank = await Within(
        unified.RerankAsync("cohere", baseUrl, "co-key", "rerank-v3.5", "what is lingxi?", new[] { "a", "b" }),
        30,
        "cohere rerank");
    Check(rerank.Results.Count == 2, $"rerank results: {rerank.Results.Count}");
    Check(rerank.Results[0].Index == 1, $"rerank top index: {rerank.Results[0].Index}");
    Check(Math.Abs(rerank.Results[0].RelevanceScore - 0.94) < 1e-9,
        $"rerank top score: {rerank.Results[0].RelevanceScore}");

    // ---- Unsupported feature ----------------------------------------------
    var unsupported = false;
    try
    {
        await unified.RerankAsync("openai_chat_completions", baseUrl, "sk-test", "gpt-4o-mini", "q", new[] { "a" });
    }
    catch (UnsupportedFeatureException error)
    {
        unsupported = true;
        Check(error.Protocol == "openai_chat_completions", $"unsupported protocol: {error.Protocol}");
        Check(error.Feature.Length > 0 && error.Feature != "unknown", $"unsupported feature: {error.Feature}");
        Check(error.Message.Contains("unsupported feature", StringComparison.Ordinal), $"message: {error.Message}");
    }
    Check(unsupported, "rerank on openai_chat_completions must raise UnsupportedFeatureException");

    // ---- Redirect block ----------------------------------------------------
    var redirectBlocked = false;
    try
    {
        await unified.CompleteTextAsync("cohere", baseUrl + "/redir", "co-key", "command-a", "hi");
    }
    catch (InvalidOperationException error)
    {
        redirectBlocked = error.Message.Contains("redirect", StringComparison.OrdinalIgnoreCase);
    }
    Check(redirectBlocked, "a 302 from the upstream must be blocked");

    // ---- LMP translation surface ------------------------------------------
    var translated = unified.TranslateRequest("openai_chat_completions", UnifiedClient.TextRequest("gpt-4o-mini", "hi"));
    Check(translated.GetProperty("model").GetString() == "gpt-4o-mini", "translate_request should keep the model");

    // ---- Native surface ----------------------------------------------------
    using (var native = new NativeClient())
    {
        var prepared = native.Prepare(new NativeRequest
        {
            Protocol = "aws_bedrock",
            BaseUrl = baseUrl,
            ApiKey = "AKID:SECRET",
            Model = "anthropic.claude-3-5-sonnet",
            Payload = JsonDocument.Parse("{\"messages\":[{\"role\":\"user\",\"content\":[{\"text\":\"hi\"}]}]}").RootElement.Clone(),
        });
        Check(prepared.Url.EndsWith("/model/anthropic.claude-3-5-sonnet/converse", StringComparison.Ordinal),
            $"bedrock url: {prepared.Url}");
        var authorization = prepared.Headers["Authorization"];
        Check(authorization.StartsWith("AWS4-HMAC-SHA256", StringComparison.Ordinal), $"sigv4 header: {authorization}");
        Check(!authorization.Contains("SECRET", StringComparison.Ordinal), "sigv4 must not leak the secret");
        Check(prepared.Framing == "amazon_event_stream", $"bedrock framing: {prepared.Framing}");

        using var response = await native.PostJsonAsync(prepared.Url, prepared.Headers, prepared.Body);
        Check(response.RootElement.GetRawText().Contains("Hello from AWS Bedrock", StringComparison.Ordinal),
            "bedrock native response body");
        Check(server.LastBedrockAuthorization?.StartsWith("AWS4-HMAC-SHA256", StringComparison.Ordinal) == true,
            "the wire request must carry the SigV4 header");

        var chatPrepared = native.Prepare(new NativeRequest
        {
            Protocol = "openai_chat_completions",
            BaseUrl = baseUrl,
            ApiKey = "sk-test",
            Payload = JsonDocument.Parse("{\"model\":\"gpt-4o-mini\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}").RootElement.Clone(),
        });
        Check(chatPrepared.Headers["Authorization"] == "Bearer sk-test", "openai bearer header");

        // Raw provider frames, incrementally.
        var frames = new List<NativeStreamEvent>();
        await foreach (var frame in native.StreamRawAsync(new NativeRequest
                       {
                           Protocol = "openai_chat_completions",
                           BaseUrl = baseUrl,
                           ApiKey = "sk-test",
                           Payload = JsonDocument.Parse("{\"model\":\"gpt-4o-mini\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}").RootElement.Clone(),
                       }, ct: new CancellationTokenSource(TimeSpan.FromSeconds(30)).Token))
        {
            frames.Add(frame);
        }
        var dataFrames = frames.Where(f => f.Data != "[DONE]").ToArray();
        Check(dataFrames.Length == 2, $"raw stream frames: {dataFrames.Length}");
        Check(frames.Any(f => f.Data == "[DONE]"), "raw stream should deliver the [DONE] sentinel");
        foreach (var frame in dataFrames)
        {
            Check(!string.IsNullOrEmpty(frame.Data), "raw frame data");
            Check(!string.IsNullOrEmpty(frame.Data), "raw frame data");
            using var parsed = frame.ParseData();
            Check(parsed is not null, "raw frame data should be JSON");
            Check(parsed!.RootElement.TryGetProperty("id", out _), "raw frame should be an OpenAI chunk");
        }
        // Batch normalizer: the same frames can be normalized in one call.
        var eventsJson = "[" + string.Join(",", dataFrames.Select(f =>
            JsonSerializer.Serialize(new { @event = f.Event, data = f.Data, id = f.Id }))) + "]";
        var normalized = unified.NormalizeStreamEvents("openai_chat_completions", eventsJson);
        var normalizedEvents = normalized.Select(LmpStreamEvent.FromJson).ToArray();
        var normalizedText = string.Concat(normalizedEvents.Where(e => e.Kind == LmpStreamEventKind.TextDelta).Select(e => e.Delta));
        Check(normalizedText == "Hello world", $"normalized stream text: {normalizedText}");
        Check(normalizedEvents.LastOrDefault(e => e.Kind == LmpStreamEventKind.Usage)?.Usage?.TotalTokens == 12,
            "normalized stream usage should be 12");

        // Cancellation mid-stream.
        var cancellationRequest = new JsonObjectBuilder()
            .Add("protocol", "openai_chat_completions")
            .Add("baseUrl", baseUrl + "/cancel")
            .Add("apiKey", "sk-test")
            .Add("unified", false)
            .AddPayload("{\"model\":\"gpt-4o-mini\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}")
            .Build();
        var session = StreamSession.Open(CoreBridge.Shared, cancellationRequest);
        try
        {
            var first = session.Next(20_000);
            Check(first is not null, "cancellation stream should deliver one frame first");
            await server.StalledAfterFirstFrame.Task.WaitAsync(TimeSpan.FromSeconds(10));
            session.Cancel();
            var code = string.Empty;
            var cancelled = false;
            for (var i = 0; i < 10 && !cancelled; i++)
            {
                try
                {
                    if (session.Next(5_000) is null)
                    {
                        break;
                    }
                }
                catch (CoreException error)
                {
                    cancelled = error.Code?.Contains("cancel", StringComparison.OrdinalIgnoreCase) == true;
                    code = error.Code ?? error.Message;
                }
            }
            Check(cancelled, $"cancelled stream should report a cancel code, got '{code}'");
        }
        finally
        {
            session.Cancel();
            session.Close();
        }
        // Cancelling the IAsyncEnumerable mid-stream surfaces OperationCanceledException.
        using (var streamCts = new CancellationTokenSource())
        {
            var unifiedCancelled = false;
            var sawEvent = false;
            try
            {
                await foreach (var @event in unified.StreamAsync("openai_chat_completions", baseUrl + "/cancel", "sk-test", streamRequest, ct: streamCts.Token))
                {
                    sawEvent = true;
                    streamCts.Cancel();
                }
            }
            catch (OperationCanceledException)
            {
                unifiedCancelled = true;
            }
            Check(sawEvent, "unified cancellation should deliver at least one event first");
            Check(unifiedCancelled, "cancelling the unified stream token must raise OperationCanceledException");
        }

        // Insecure endpoints are rejected by the core.
        var insecure = false;
        try
        {
            native.Prepare(new NativeRequest
            {
                Protocol = "openai_chat_completions",
                BaseUrl = "http://example.com/v1",
                ApiKey = "sk-test",
                Payload = JsonDocument.Parse("{}").RootElement.Clone(),
            });
        }
        catch (CoreException error)
        {
            insecure = error.Code == "invalid_endpoint";
        }
        Check(insecure, "http://example.com/v1 must be rejected with invalid_endpoint");
    }

    // ---- Catalog -----------------------------------------------------------
    var cacheDirectory = Path.Combine(Path.GetTempPath(), "lingxi-csharp-catalog-" + Guid.NewGuid().ToString("N"));
    try
    {
        var loader = new CatalogLoader(new CatalogLoader.Config
        {
            Endpoint = baseUrl + "/models.json",
            CacheDirectory = cacheDirectory,
            MaxAge = TimeSpan.FromMinutes(30),
        });
        var catalog = await Within(loader.LoadAsync(), 30, "catalog load");
        Check(catalog.Revision.CatalogRevisionId == "conf-rev-001", $"catalogRevision: {catalog.Revision.CatalogRevisionId}");
        Check(catalog.TotalModels == 5, $"totalModels: {catalog.TotalModels}");
        Check(catalog.Revision.TotalProviders == 3, $"totalProviders: {catalog.Revision.TotalProviders}");
        Check(server.CatalogHits == 1, $"catalog hits after first load: {server.CatalogHits}");

        var second = await Within(loader.LoadAsync(), 30, "cached catalog load");
        Check(server.CatalogHits == 1, $"a fresh catalog must not refetch, hits: {server.CatalogHits}");
        Check(second.TotalModels == 5, "cached catalog should keep every model");

        var revalidated = await Within(loader.LoadAsync(forceRefresh: true), 30, "revalidated catalog load");
        Check(server.CatalogHits == 2, $"forceRefresh should revalidate, hits: {server.CatalogHits}");
        Check(revalidated.Revision.CatalogRevisionId == "conf-rev-001", "304 must keep the cached revision");
        var metaPath = Path.Combine(cacheDirectory, "models.json.meta.json");
        Check(File.Exists(Path.Combine(cacheDirectory, "models.json")), "catalog cache file must be written");
        using var metaDoc = JsonDocument.Parse(await File.ReadAllTextAsync(metaPath));
        Check(metaDoc.RootElement.GetProperty("etag").GetString() == "\"rev-1\"", "meta.json must persist the ETag");
        Check(metaDoc.RootElement.GetProperty("fetchedAt").GetString() is { Length: > 0 }, "meta.json must persist fetchedAt");

        server.CatalogMode = 1;
        var staleLoader = new CatalogLoader(new CatalogLoader.Config
        {
            Endpoint = baseUrl + "/models.json",
            CacheDirectory = cacheDirectory,
            MaxAge = TimeSpan.Zero,
        });
        var stale = await Within(staleLoader.LoadAsync(), 30, "stale fallback");
        Check(stale.TotalModels == 5, $"a 503 with a disk cache must fall back, models: {stale.TotalModels}");

        var failingLoader = new CatalogLoader(new CatalogLoader.Config
        {
            Endpoint = baseUrl + "/models.json",
            CacheDirectory = null,
            MaxAge = TimeSpan.Zero,
        });
        var failed = false;
        try
        {
            await failingLoader.LoadAsync();
        }
        catch (CatalogException error)
        {
            failed = error.Kind == CatalogErrorKind.UnexpectedStatus && error.StatusCode == 503;
        }
        Check(failed, "a 503 without any cache must raise CatalogException(UnexpectedStatus, 503)");
    }
    finally
    {
        if (Directory.Exists(cacheDirectory))
        {
            Directory.Delete(cacheDirectory, recursive: true);
        }
    }

    // ---- Catalog model API -------------------------------------------------
    var fixture = await File.ReadAllBytesAsync(Path.Combine(root, "conformance", "catalog", "fixtures", "v2_catalog.json"));
    var typed = LingXiModelCatalog.Decode(fixture);
    var expectations = JsonDocument.Parse(await File.ReadAllTextAsync(Path.Combine(root, "conformance", "catalog", "expectations.json"))).RootElement;

    static string[] Qualified(IEnumerable<CatalogModel> models) => models.Select(m => m.QualifiedId).ToArray();
    static void CheckSequence(IEnumerable<string> actual, JsonElement expected, string what)
    {
        var expectedList = expected.EnumerateArray().Select(e => e.GetString()).ToArray();
        var actualList = actual.ToArray();
        if (!expectedList.SequenceEqual(actualList))
        {
            throw new Exception($"{what}: expected [{string.Join(", ", expectedList)}], got [{string.Join(", ", actualList)}]");
        }
    }

    Check(typed.Revision.SchemaVersion == expectations.GetProperty("schemaVersion").GetString(), "schema version");
    Check(typed.Revision.CatalogRevisionId == expectations.GetProperty("catalogRevision").GetString(), "catalog revision");
    Check(typed.Revision.TotalProviders == expectations.GetProperty("totalProviders").GetInt32(), "totalProviders");
    Check(typed.Revision.TotalModels == expectations.GetProperty("totalModels").GetInt32(), "totalModels");
    CheckSequence(typed.SortedProviders().Select(p => p.Id), expectations.GetProperty("providerOrder"), "provider order");

    var featured = expectations.GetProperty("featuredProviderOrder");
    CheckSequence(
        typed.SortedProviders(featured.GetProperty("featured").EnumerateArray().Select(e => e.GetString()!).ToArray()).Select(p => p.Id),
        featured.GetProperty("expected"),
        "featured provider order");

    CheckSequence(typed.ModelsForProvider("prov10").Select(m => m.Id), expectations.GetProperty("prov10ModelOrder"), "prov10 model order");
    CheckSequence(typed.ModelsForProvider("deepseek").Select(m => m.Id), expectations.GetProperty("deepseekModelOrder"), "deepseek model order");
    CheckSequence(Qualified(typed.AllModels()), expectations.GetProperty("allModelQualifiedOrder"), "all model order");

    var search = expectations.GetProperty("search");
    CheckSequence(Qualified(typed.Search("gpt", 1)), search.GetProperty("gptLimit1"), "search gpt limit 1");
    CheckSequence(Qualified(typed.Search("deepseek")), search.GetProperty("deepseek"), "search deepseek");

    var filters = expectations.GetProperty("filters");
    CheckSequence(Qualified(typed.ModelsMatching(new ModelFilter { Vision = true })), filters.GetProperty("vision"), "vision filter");
    CheckSequence(
        Qualified(typed.ModelsMatching(new ModelFilter { MinimumContextWindow = 200_000 })),
        filters.GetProperty("minContext200k"),
        "min context filter");
    CheckSequence(
        Qualified(typed.ModelsMatching(new ModelFilter { ProviderId = "deepseek" })),
        filters.GetProperty("deepseekDefault"),
        "deepseek default filter");
    CheckSequence(
        Qualified(typed.ModelsMatching(new ModelFilter { ProviderId = "deepseek", IncludeDeprecated = true })),
        filters.GetProperty("deepseekIncludeDeprecated"),
        "deepseek include-deprecated filter");

    var provider10 = typed.Provider("prov10") ?? throw new Exception("prov10 missing");
    Check(provider10.BaseUrl == "https://api.prov10.example/v1", $"prov10 baseUrl: {provider10.BaseUrl}");
    Check(provider10.EnvironmentVariableNames.SequenceEqual(new[] { "PROV10_API_KEY" }), "prov10 env");
    var gpt10 = typed.Model("prov10", "gpt-10") ?? throw new Exception("gpt-10 missing");
    Check(gpt10.Pricing.Input == 0.0 && !gpt10.Pricing.IsUnknown, "gpt-10 pricing.input must be a known 0.0");
    Check(gpt10.Capabilities.SupportsTemperature == false, "gpt-10 supportsTemperature must be false");
    Check(gpt10.Capabilities.Vision && gpt10.Capabilities.AudioInput, "gpt-10 vision/audio capabilities");
    Check(gpt10.Capabilities.ReasoningEfforts.SequenceEqual(new[] { "low", "medium", "high" }), "gpt-10 reasoning efforts");
    Check(gpt10.Fields.TryGetValue("experimental", out var experimental)
          && experimental is IReadOnlyDictionary<string, object?> experimentalMap
          && experimentalMap.TryGetValue("warmup", out var warmup)
          && Equals(warmup, true),
        "gpt-10 must preserve unknown fields");
    Check(typed.Model("prov10/gpt-10")?.Id == "gpt-10", "qualified model lookup");

    var gpt5 = typed.Model("prov10", "gpt-5") ?? throw new Exception("gpt-5 missing");
    Check(gpt5.ContextWindow == 0, $"gpt-5 context: {gpt5.ContextWindow}");
    Check(gpt5.Pricing.IsUnknown, "gpt-5 pricing must stay unknown");
    Check(gpt5.Capabilities.SupportsTemperature is null, "gpt-5 supportsTemperature must stay null");
    Check(!gpt5.Capabilities.StructuredOutput, "gpt-5 structured output");

    var legacy = LingXiModelCatalog.Decode(await File.ReadAllBytesAsync(Path.Combine(root, "conformance", "catalog", "fixtures", "v1_legacy.json")));
    Check(legacy.Revision.SchemaVersion == "1.0", $"legacy schema: {legacy.Revision.SchemaVersion}");
    Check(legacy.Model("ollama", "llama3")?.ContextWindow == 8192, "legacy ollama/llama3 context");

    var malformed = LingXiModelCatalog.Decode(await File.ReadAllBytesAsync(Path.Combine(root, "conformance", "catalog", "fixtures", "malformed_entry.json")));
    Check(malformed.Providers.Count == 1, $"malformed providers: {malformed.Providers.Count}");
    Check(malformed.Model("p", "published-key") is not null, "malformed: published-key model present");
    Check(malformed.Model("p", "internal-id") is null, "malformed: internal-id model absent");

    var schemaRejected = false;
    try
    {
        LingXiModelCatalog.Decode(await File.ReadAllBytesAsync(Path.Combine(root, "conformance", "catalog", "fixtures", "unsupported_schema.json")));
    }
    catch (CatalogException error)
    {
        schemaRejected = error.Kind == CatalogErrorKind.UnsupportedSchemaVersion && error.SchemaVersion == "3.0";
    }
    Check(schemaRejected, "schemaVersion 3.0 must raise UnsupportedSchemaVersion");

    var providersRejected = false;
    try
    {
        LingXiModelCatalog.Decode(await File.ReadAllBytesAsync(Path.Combine(root, "conformance", "catalog", "fixtures", "broken_empty_providers.json")));
    }
    catch (CatalogException error)
    {
        providersRejected = error.Kind == CatalogErrorKind.InvalidCatalog;
    }
    Check(providersRejected, "empty providers must raise InvalidCatalog");

    // The legacy ModelCatalog.Decode API keeps working on the same document.
    var legacyView = ModelCatalog.Decode(await File.ReadAllTextAsync(Path.Combine(root, "conformance", "catalog", "fixtures", "v2_catalog.json")));
    Check(legacyView.SchemaVersion == "2.0", $"ModelCatalog schema: {legacyView.SchemaVersion}");
    Check(legacyView.Providers.ContainsKey("prov10"), "ModelCatalog providers");
    Check(legacyView.Models().Count == 5, $"ModelCatalog models: {legacyView.Models().Count}");
}

Console.WriteLine("C# 4-module SDK tests passed (Catalog, Registry, Native, Unified on the Rust core).");

/// <summary>Small mutable JSON builder for the raw stream session request.</summary>
internal sealed class JsonObjectBuilder
{
    private readonly System.Text.Json.Nodes.JsonObject _json = new();

    public JsonObjectBuilder Add(string name, string value)
    {
        _json[name] = value;
        return this;
    }

    public JsonObjectBuilder Add(string name, bool value)
    {
        _json[name] = value;
        return this;
    }

    public JsonObjectBuilder Add(string name, long value)
    {
        _json[name] = value;
        return this;
    }

    /// <summary>Adds a property whose value is an embedded JSON document.</summary>
    public JsonObjectBuilder Add(string name, System.Text.Json.JsonElement value)
    {
        _json[name] = System.Text.Json.Nodes.JsonNode.Parse(value.GetRawText());
        return this;
    }

    public JsonObjectBuilder AddPayload(string payloadJson)
    {
        _json["payload"] = System.Text.Json.Nodes.JsonNode.Parse(payloadJson);
        return this;
    }

    public string Build() => _json.ToJsonString();
}
