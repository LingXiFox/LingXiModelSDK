using System.Globalization;
using System.Net;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;

namespace LingXiModelSDK.Catalog;

/// <summary>
/// Loads <c>models.json</c> with a memory and on-disk cache.
///
/// Conditional requests (<c>If-None-Match</c>/304), a maximum age and a stale
/// fallback keep the catalog usable offline; the loader never needs an API key.
/// </summary>
public sealed class CatalogLoader
{
    public const string DefaultEndpoint = "https://models.lingxifox.cn/models.json";

    private readonly object _gate = new();
    private CachedCatalog? _memory;

    public CatalogLoader(Config? config = null, Transport? transport = null)
    {
        Options = config ?? new Config();
        Fetcher = transport ?? HttpTransportAsync;
    }

    public Config Options { get; }

    /// <summary>Injected network transport; the default uses <see cref="HttpClient"/>.</summary>
    public Transport Fetcher { get; }

    public sealed record Config
    {
        public string Endpoint { get; init; } = DefaultEndpoint;

        public TimeSpan? MaxAge { get; init; } = TimeSpan.FromMinutes(30);

        public string? CacheDirectory { get; init; }

        public TimeSpan? RequestTimeout { get; init; } = TimeSpan.FromSeconds(30);

        public string UserAgent { get; init; } = "LingXiModelSDK-Csharp/2.0";
    }

    public sealed record HttpReply(int StatusCode, byte[] Body, string? ETag);

    public delegate Task<HttpReply> Transport(
        string url,
        IReadOnlyDictionary<string, string> headers,
        TimeSpan timeout,
        CancellationToken cancellationToken);

    private sealed record CachedCatalog(LingXiModelCatalog Catalog, DateTimeOffset FetchedAt, string? ETag)
    {
        public bool IsFresh(TimeSpan? maxAge, DateTimeOffset now)
            => maxAge is { } age && age > TimeSpan.Zero && now - FetchedAt < age;
    }

    /// <summary>The last successfully loaded catalog, without any network access.</summary>
    public LingXiModelCatalog? Cached() => CachedSnapshot()?.Catalog;

    public async Task<LingXiModelCatalog> LoadAsync(bool forceRefresh = false, CancellationToken cancellationToken = default)
    {
        var cached = CachedSnapshot();
        if (!forceRefresh && cached is not null && cached.IsFresh(Options.MaxAge, DateTimeOffset.UtcNow))
        {
            return cached.Catalog;
        }

        var headers = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            ["Accept"] = "application/json",
            ["User-Agent"] = Options.UserAgent,
        };
        if (!string.IsNullOrEmpty(cached?.ETag))
        {
            headers["If-None-Match"] = cached!.ETag!;
        }

        HttpReply reply;
        try
        {
            reply = await Fetcher(Options.Endpoint, headers, Options.RequestTimeout ?? TimeSpan.FromSeconds(30), cancellationToken)
                .ConfigureAwait(false);
        }
        catch (Exception error) when (error is not OperationCanceledException)
        {
            if (cached is not null)
            {
                return cached.Catalog;
            }
            throw new CatalogException(CatalogErrorKind.Transport, "Catalog transport failed: " + error.Message, innerException: error);
        }

        if (reply.StatusCode == 304 && cached is not null)
        {
            var refreshed = new CachedCatalog(
                cached.Catalog,
                DateTimeOffset.UtcNow,
                string.IsNullOrEmpty(reply.ETag) ? cached.ETag : reply.ETag);
            Store(refreshed);
            PersistMeta(refreshed);
            return refreshed.Catalog;
        }

        if (reply.StatusCode is < 200 or > 299)
        {
            if (cached is not null)
            {
                return cached.Catalog;
            }
            throw new CatalogException(
                CatalogErrorKind.UnexpectedStatus,
                "Unexpected HTTP status: " + reply.StatusCode,
                reply.StatusCode);
        }

        LingXiModelCatalog catalog;
        try
        {
            catalog = CatalogDecoder.Decode(reply.Body);
        }
        catch (CatalogException error)
        {
            if (cached is not null)
            {
                return cached.Catalog;
            }
            throw new CatalogException(error.Kind, error.Message, error.StatusCode, error.SchemaVersion, error);
        }

        var fresh = new CachedCatalog(catalog, DateTimeOffset.UtcNow, reply.ETag);
        Store(fresh);
        PersistCatalog(reply.Body);
        PersistMeta(fresh);
        return catalog;
    }

    private CachedCatalog? CachedSnapshot()
    {
        lock (_gate)
        {
            if (_memory is not null)
            {
                return _memory;
            }
        }

        var directory = Options.CacheDirectory;
        if (string.IsNullOrEmpty(directory))
        {
            return null;
        }

        try
        {
            var catalogPath = Path.Combine(directory, "models.json");
            if (!File.Exists(catalogPath))
            {
                return null;
            }
            var catalog = CatalogDecoder.Decode(File.ReadAllBytes(catalogPath));
            var fetchedAt = catalog.Revision.GeneratedAt ?? DateTimeOffset.UnixEpoch;
            string? etag = null;
            var metaPath = Path.Combine(directory, "models.json.meta.json");
            if (File.Exists(metaPath))
            {
                using var meta = JsonDocument.Parse(File.ReadAllBytes(metaPath));
                etag = meta.RootElement.OptString("etag");
                fetchedAt = CatalogDecoder.ParseTimestamp(meta.RootElement.OptString("fetchedAt")) ?? fetchedAt;
            }
            return new CachedCatalog(catalog, fetchedAt, etag);
        }
        catch (Exception)
        {
            return null;
        }
    }

    private void Store(CachedCatalog cached)
    {
        lock (_gate)
        {
            _memory = cached;
        }
    }

    private void PersistCatalog(byte[] body)
    {
        var directory = Options.CacheDirectory;
        if (string.IsNullOrEmpty(directory))
        {
            return;
        }
        WriteAtomic(Path.Combine(directory, "models.json"), body);
    }

    private void PersistMeta(CachedCatalog cached)
    {
        var directory = Options.CacheDirectory;
        if (string.IsNullOrEmpty(directory))
        {
            return;
        }
        var json = new StringBuilder();
        json.Append('{');
        json.Append("\"etag\":");
        json.Append(cached.ETag is null ? "null" : JsonSerializer.Serialize(cached.ETag));
        json.Append(",\"fetchedAt\":");
        json.Append(JsonSerializer.Serialize(
            cached.FetchedAt.ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'", CultureInfo.InvariantCulture)));
        json.Append('}');
        WriteAtomic(Path.Combine(directory, "models.json.meta.json"), Encoding.UTF8.GetBytes(json.ToString()));
    }

    private static void WriteAtomic(string path, byte[] body)
    {
        try
        {
            var directory = Path.GetDirectoryName(path);
            if (!string.IsNullOrEmpty(directory))
            {
                Directory.CreateDirectory(directory);
            }
            var temporary = Path.Combine(directory ?? ".", ".cache-" + Guid.NewGuid().ToString("N") + ".tmp");
            File.WriteAllBytes(temporary, body);
            File.Move(temporary, path, overwrite: true);
        }
        catch (IOException)
        {
            // A cache write is best effort; the catalog itself stays usable.
        }
        catch (UnauthorizedAccessException)
        {
        }
    }

    private async Task<HttpReply> HttpTransportAsync(
        string url,
        IReadOnlyDictionary<string, string> headers,
        TimeSpan timeout,
        CancellationToken cancellationToken)
    {
        using var handler = new HttpClientHandler { AllowAutoRedirect = true };
        using var client = new HttpClient(handler) { Timeout = timeout };
        using var request = new HttpRequestMessage(HttpMethod.Get, url);
        foreach (var (name, value) in headers)
        {
            request.Headers.TryAddWithoutValidation(name, value);
        }
        using var response = await client.SendAsync(request, cancellationToken).ConfigureAwait(false);
        var body = await response.Content.ReadAsByteArrayAsync(cancellationToken).ConfigureAwait(false);
        return new HttpReply(
            (int)response.StatusCode,
            body,
            response.Headers.TryGetValues("ETag", out var etags) ? etags.FirstOrDefault() : null);
    }
}
