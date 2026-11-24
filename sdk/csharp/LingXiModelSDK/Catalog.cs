using System.Text.Json;

namespace LingXiModelSDK.Catalog;

public sealed record CatalogModelSummary(string Id, string ProviderId, string Name, long? ContextLimit, long? OutputLimit);

/// <summary>Zero-inference-dependency Model Catalog reader for schema 2.0.</summary>
public sealed class ModelCatalog
{
    public string SchemaVersion { get; }
    public IReadOnlyDictionary<string, JsonElement> Providers { get; }

    private ModelCatalog(string schemaVersion, Dictionary<string, JsonElement> providers)
    {
        SchemaVersion = schemaVersion;
        Providers = providers;
    }

    public static ModelCatalog Decode(string json)
    {
        using var doc = JsonDocument.Parse(json);
        var root = doc.RootElement;
        if (!root.TryGetProperty("schemaVersion", out var sv) || sv.GetString() != "2.0")
        {
            throw new InvalidOperationException("Unsupported or missing catalog schemaVersion (expected 2.0)");
        }
        var providers = new Dictionary<string, JsonElement>(StringComparer.Ordinal);
        if (root.TryGetProperty("providers", out var pObj) && pObj.ValueKind == JsonValueKind.Object)
        {
            foreach (var prop in pObj.EnumerateObject())
            {
                providers[prop.Name] = prop.Value.Clone();
            }
        }
        return new ModelCatalog("2.0", providers);
    }
}
