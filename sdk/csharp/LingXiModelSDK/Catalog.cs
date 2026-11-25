using System.Text.Json;

namespace LingXiModelSDK;

/// <summary>
/// Legacy read-only catalog view kept for source compatibility.
///
/// <see cref="Decode"/> now delegates to the typed <see cref="Catalog.LingXiModelCatalog"/>
/// decoder, so the same validation rules apply, while the raw provider entries stay
/// available as <see cref="JsonElement"/> values.
/// </summary>
public sealed class ModelCatalog
{
    private ModelCatalog(string schemaVersion, IReadOnlyDictionary<string, JsonElement> providers)
    {
        SchemaVersion = schemaVersion;
        Providers = providers;
    }

    public string SchemaVersion { get; }

    public IReadOnlyDictionary<string, JsonElement> Providers { get; }

    /// <summary>Typed view of the same document.</summary>
    public Catalog.LingXiModelCatalog Typed { get; private init; } = null!;

    public static ModelCatalog Decode(string json)
    {
        var typed = Catalog.LingXiModelCatalog.Decode(json);
        return Build(typed, json);
    }

    private static ModelCatalog Build(Catalog.LingXiModelCatalog typed, string json)
    {
        using var document = JsonDocument.Parse(json);
        var providers = new Dictionary<string, JsonElement>(StringComparer.Ordinal);
        if (document.RootElement.ValueKind == JsonValueKind.Object
            && document.RootElement.TryGetProperty("providers", out var providersElement)
            && providersElement.ValueKind == JsonValueKind.Object)
        {
            foreach (var entry in providersElement.EnumerateObject())
            {
                providers[entry.Name] = entry.Value.Clone();
            }
        }

        return new ModelCatalog(typed.Revision.SchemaVersion, providers) { Typed = typed };
    }

    /// <summary>Summaries of every model in the catalog, in stable order.</summary>
    public IReadOnlyList<CatalogModelSummary> Models()
        => Typed.AllModels()
            .Select(model => new CatalogModelSummary(
                model.Id,
                model.ProviderId,
                model.Name,
                model.ContextWindow,
                model.MaxOutputTokens))
            .ToArray();
}

/// <summary>Flat summary of one catalog model.</summary>
public sealed record CatalogModelSummary(
    string Id,
    string ProviderId,
    string Name,
    long? ContextLimit,
    long? OutputLimit);
