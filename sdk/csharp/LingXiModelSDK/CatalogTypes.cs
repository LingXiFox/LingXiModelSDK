using System.Globalization;

namespace LingXiModelSDK.Catalog;

/// <summary>Publication status of a catalog model.</summary>
public enum ModelStatus
{
    Active,
    Preview,
    Deprecated,
    Retired,
    Unknown,
}

public static class ModelStatusKinds
{
    public static ModelStatus FromRaw(string? raw) => raw switch
    {
        "deprecated" or "retired" => ModelStatus.Deprecated,
        "beta" or "preview" => ModelStatus.Preview,
        "active" => ModelStatus.Active,
        _ => ModelStatus.Unknown,
    };

    /// <summary>True when callers may still select this model.</summary>
    public static bool IsSelectable(this ModelStatus status) => status is not (ModelStatus.Deprecated or ModelStatus.Retired);
}

/// <summary>Token limits published for one model; absent values stay null.</summary>
public sealed record ModelLimits(long? Context, long? Input, long? Output);

/// <summary>Capabilities inferred from the published flags and modalities.</summary>
public sealed record ModelCapabilities(
    bool Reasoning,
    IReadOnlyList<string> ReasoningEfforts,
    bool ToolCalling,
    bool StructuredOutput,
    bool Attachments,
    bool Vision,
    bool AudioInput,
    bool AudioOutput,
    IReadOnlyList<string> InputModalities,
    IReadOnlyList<string> OutputModalities,
    bool OpenWeights,
    bool? SupportsTemperature);

/// <summary>Per-million-token pricing; all null when the catalog publishes none.</summary>
public sealed record ModelPricing(
    double? Input,
    double? Output,
    double? CacheRead,
    double? CacheWrite,
    double? Reasoning,
    double? InputAudio,
    double? OutputAudio)
{
    public static readonly ModelPricing Unknown = new(null, null, null, null, null, null, null);

    public bool IsUnknown =>
        Input is null && Output is null && CacheRead is null && CacheWrite is null
        && Reasoning is null && InputAudio is null && OutputAudio is null;
}

/// <summary>One model entry of the published catalog.</summary>
public sealed record CatalogModel(
    string Id,
    string ProviderId,
    string Name,
    string? Family,
    string? Overview,
    string? ReleaseDate,
    string? LastUpdated,
    string? KnowledgeCutoff,
    string? CanonicalModelId,
    ModelStatus Status,
    ModelLimits Limits,
    ModelCapabilities Capabilities,
    ModelPricing Pricing,
    IReadOnlyDictionary<string, object?> Fields)
{
    public string QualifiedId => ProviderId + "/" + Id;

    public long? ContextWindow => Limits.Context;

    public long? MaxOutputTokens => Limits.Output;
}

/// <summary>One provider entry with its models in stable order.</summary>
public sealed record CatalogProvider(
    string Id,
    string Name,
    string? BaseUrl,
    string? DocumentationUrl,
    IReadOnlyList<string> EnvironmentVariableNames,
    IReadOnlyList<CatalogModel> Models,
    IReadOnlyDictionary<string, object?> Fields)
{
    public int ModelCount => Models.Count;
}

/// <summary>Document-level catalog metadata.</summary>
public sealed record CatalogRevision(
    string SchemaVersion,
    string CatalogRevisionId,
    string? CatalogHash,
    string? Source,
    string? SourceUrl,
    string? SourceHash,
    DateTimeOffset? GeneratedAt,
    DateTimeOffset? SourceFetchedAt,
    int TotalProviders,
    int TotalModels)
{
    public CatalogRevision WithTotals(int totalProviders, int totalModels)
        => this with { TotalProviders = totalProviders, TotalModels = totalModels };
}

/// <summary>Declarative filter understood by <see cref="LingXiModelCatalog.ModelsMatching"/>.</summary>
public sealed record ModelFilter
{
    public string? ProviderId { get; init; }

    public string? Query { get; init; }

    public bool? Reasoning { get; init; }

    public bool? ToolCalling { get; init; }

    public bool? StructuredOutput { get; init; }

    public bool? Vision { get; init; }

    public bool? Attachments { get; init; }

    public bool? OpenWeights { get; init; }

    public long? MinimumContextWindow { get; init; }

    public long? MinimumMaxOutputTokens { get; init; }

    public bool IncludeDeprecated { get; init; }

    public IReadOnlyList<string> FeaturedProviderIds { get; init; } = Array.Empty<string>();
}
