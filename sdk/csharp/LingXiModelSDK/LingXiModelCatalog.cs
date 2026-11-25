using System.Globalization;

namespace LingXiModelSDK.Catalog;

/// <summary>
/// Immutable model catalog with lookup, search and filtering.
///
/// This entry point is independent of the provider clients: it needs no API key
/// and no Rust core.
/// </summary>
public sealed class LingXiModelCatalog
{
    private readonly IReadOnlyList<CatalogProvider> _providers;
    private readonly byte[]? _rawDocument;

    public LingXiModelCatalog(CatalogRevision revision, IReadOnlyList<CatalogProvider> providers, byte[]? rawDocument)
    {
        Revision = revision;
        _providers = providers.ToArray();
        _rawDocument = rawDocument;
    }

    /// <summary>Document metadata; total counts default to the decoded contents.</summary>
    public CatalogRevision Revision { get; }

    public IReadOnlyList<CatalogProvider> Providers => _providers;

    /// <summary>The exact bytes this catalog was decoded from, if any.</summary>
    public byte[]? RawDocument => _rawDocument?.ToArray();

    public int TotalModels => _providers.Sum(provider => provider.ModelCount);

    public static LingXiModelCatalog Decode(byte[] data) => CatalogDecoder.Decode(data);

    public static LingXiModelCatalog Decode(string json) => CatalogDecoder.Decode(System.Text.Encoding.UTF8.GetBytes(json));

    public CatalogProvider? Provider(string? providerId)
    {
        var key = Norm(providerId);
        return _providers.FirstOrDefault(provider => Norm(provider.Id) == key);
    }

    public CatalogModel? Model(string? providerId, string? modelId)
    {
        var provider = Provider(providerId);
        if (provider is null)
        {
            return null;
        }
        var key = Norm(modelId);
        return provider.Models.FirstOrDefault(model => Norm(model.Id) == key);
    }

    /// <summary>Looks a model up by id, accepting a <c>provider/model</c> qualified id.</summary>
    public CatalogModel? Model(string id)
    {
        var slash = id.IndexOf('/');
        if (slash >= 0)
        {
            return Model(id[..slash], id[(slash + 1)..]);
        }
        var key = Norm(id);
        foreach (var provider in _providers)
        {
            foreach (var model in provider.Models)
            {
                if (Norm(model.Id) == key)
                {
                    return model;
                }
            }
        }
        return null;
    }

    public IReadOnlyList<CatalogProvider> SortedProviders(IReadOnlyList<string>? featured = null)
        => CatalogOrdering.SortProviders(_providers, featured);

    public IReadOnlyList<CatalogModel> ModelsForProvider(string? providerId)
    {
        var provider = Provider(providerId);
        return provider is null ? Array.Empty<CatalogModel>() : CatalogOrdering.ModelsInStableOrder(provider.Models);
    }

    /// <summary>Every model, provider by provider in catalog order.</summary>
    public IReadOnlyList<CatalogModel> AllModels()
    {
        var all = new List<CatalogModel>();
        foreach (var provider in SortedProviders())
        {
            all.AddRange(CatalogOrdering.ModelsInStableOrder(provider.Models));
        }
        return all;
    }

    /// <summary>Substring search over provider and model metadata.</summary>
    public IReadOnlyList<CatalogModel> Search(string? query, int? limit = null)
    {
        var needle = Norm(query);
        if (needle.Length == 0)
        {
            var all = AllModels();
            return limit is { } cap && all.Count > cap ? all.Take(cap).ToArray() : all;
        }
        var matches = new List<CatalogModel>();
        foreach (var provider in SortedProviders())
        {
            var ordered = CatalogOrdering.ModelsInStableOrder(provider.Models);
            if (Norm(provider.Id).Contains(needle, StringComparison.Ordinal)
                || Norm(provider.Name).Contains(needle, StringComparison.Ordinal))
            {
                matches.AddRange(ordered);
            }
            else
            {
                matches.AddRange(ordered.Where(model =>
                    ContainsAny(needle, model.Id, model.Name, model.Family, model.Overview)));
            }
            if (limit is { } cap && matches.Count >= cap)
            {
                return matches.Take(cap).ToArray();
            }
        }
        return matches;
    }

    public IReadOnlyList<CatalogModel> ModelsMatching(ModelFilter filter)
    {
        var matches = new List<CatalogModel>();
        foreach (var provider in SortedProviders(filter.FeaturedProviderIds))
        {
            if (filter.ProviderId is not null && Norm(provider.Id) != Norm(filter.ProviderId))
            {
                continue;
            }
            foreach (var model in CatalogOrdering.ModelsInStableOrder(provider.Models))
            {
                if (Matches(model, provider, filter))
                {
                    matches.Add(model);
                }
            }
        }
        return matches;
    }

    private static bool Matches(CatalogModel model, CatalogProvider provider, ModelFilter filter)
    {
        if (!filter.IncludeDeprecated && !model.Status.IsSelectable())
        {
            return false;
        }
        var caps = model.Capabilities;
        if (filter.Reasoning is { } reasoning && caps.Reasoning != reasoning)
        {
            return false;
        }
        if (filter.ToolCalling is { } toolCalling && caps.ToolCalling != toolCalling)
        {
            return false;
        }
        if (filter.StructuredOutput is { } structuredOutput && caps.StructuredOutput != structuredOutput)
        {
            return false;
        }
        if (filter.Vision is { } vision && caps.Vision != vision)
        {
            return false;
        }
        if (filter.Attachments is { } attachments && caps.Attachments != attachments)
        {
            return false;
        }
        if (filter.OpenWeights is { } openWeights && caps.OpenWeights != openWeights)
        {
            return false;
        }
        if (filter.MinimumContextWindow is { } minimumContext && (model.Limits.Context ?? 0) < minimumContext)
        {
            return false;
        }
        if (filter.MinimumMaxOutputTokens is { } minimumOutput && (model.Limits.Output ?? 0) < minimumOutput)
        {
            return false;
        }
        var needle = Norm(filter.Query);
        if (needle.Length > 0
            && !ContainsAny(needle, model.Id, model.Name, model.Family, model.Overview, provider.Id, provider.Name))
        {
            return false;
        }
        return true;
    }

    internal static string Norm(string? text) => text is null ? string.Empty : text.Trim().ToLowerInvariant();

    private static bool ContainsAny(string needle, params string?[] fields)
        => fields.Any(field => field is not null && Norm(field).Contains(needle, StringComparison.Ordinal));
}
