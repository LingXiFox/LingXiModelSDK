using System.Globalization;
using System.Text.Json;

namespace LingXiModelSDK.Catalog;

/// <summary>Decodes <c>models.json</c> documents into typed catalog objects.</summary>
public static class CatalogDecoder
{
    private const string ModelStatusActive = "active";

    public static LingXiModelCatalog Decode(string json) => Decode(System.Text.Encoding.UTF8.GetBytes(json));

    public static LingXiModelCatalog Decode(byte[] data)
    {
        JsonDocument document;
        try
        {
            document = JsonDocument.Parse(data);
        }
        catch (JsonException error)
        {
            throw new CatalogException(CatalogErrorKind.InvalidCatalog, "Invalid JSON: " + error.Message, innerException: error);
        }

        using (document)
        {
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object)
            {
                throw new CatalogException(CatalogErrorKind.InvalidCatalog, "Catalog root is not an object");
            }

            if (!root.TryGetProperty("providers", out var providersElement)
                || providersElement.ValueKind != JsonValueKind.Object
                || !providersElement.EnumerateObject().Any())
            {
                throw new CatalogException(CatalogErrorKind.InvalidCatalog, "providers missing or empty");
            }

            var schemaVersion = root.OptString("schemaVersion") ?? root.OptString("version") ?? "1.0";
            var major = schemaVersion.Split('.', 2)[0];
            if (major != "1" && major != "2")
            {
                throw new CatalogException(
                    CatalogErrorKind.UnsupportedSchemaVersion,
                    "Unsupported schema version: " + schemaVersion,
                    0,
                    schemaVersion);
            }

            var providers = new List<CatalogProvider>();
            foreach (var entry in providersElement.EnumerateObject())
            {
                if (entry.Value.ValueKind != JsonValueKind.Object)
                {
                    continue;
                }
                var provider = DecodeProvider(entry.Name, entry.Value);
                if (provider is not null)
                {
                    providers.Add(provider);
                }
            }

            if (providers.Count == 0)
            {
                throw new CatalogException(CatalogErrorKind.InvalidCatalog, "No valid provider entries");
            }

            var sorted = CatalogOrdering.SortProviders(providers);
            var generatedAt = ParseTimestamp(root.OptString("generatedAt") ?? root.OptString("updatedAt"));
            var catalogRevision = root.NonEmpty("catalogRevision")
                ?? root.NonEmpty("catalogHash")
                ?? FormatTimestamp(generatedAt)
                ?? string.Empty;
            var totalProviders = root.OptLong("totalProviders") ?? sorted.Count;
            var totalModels = root.OptLong("totalModels") ?? sorted.Sum(provider => provider.ModelCount);

            var revision = new CatalogRevision(
                schemaVersion,
                catalogRevision,
                root.NonEmpty("catalogHash"),
                root.NonEmpty("source"),
                root.NonEmpty("sourceURL"),
                root.NonEmpty("sourceHash"),
                generatedAt,
                ParseTimestamp(root.OptString("sourceFetchedAt")),
                (int)totalProviders,
                (int)totalModels);

            return new LingXiModelCatalog(revision, sorted, data);
        }
    }

    private static CatalogProvider? DecodeProvider(string key, JsonElement fields)
    {
        var models = new List<CatalogModel>();
        if (fields.TryGetProperty("models", out var modelsElement) && modelsElement.ValueKind == JsonValueKind.Object)
        {
            foreach (var entry in modelsElement.EnumerateObject())
            {
                if (entry.Value.ValueKind != JsonValueKind.Object)
                {
                    continue;
                }
                var model = DecodeModel(key, entry.Name, entry.Value);
                if (model is not null)
                {
                    models.Add(model);
                }
            }
        }

        if (models.Count == 0 && !fields.TryGetProperty("id", out _))
        {
            return null;
        }

        var id = fields.OptString("id") ?? key;
        var name = fields.OptString("name") ?? id;
        return new CatalogProvider(
            id,
            name,
            fields.NonEmpty("baseURL"),
            fields.NonEmpty("doc"),
            fields.StringList("env"),
            CatalogOrdering.ModelsInStableOrder(models),
            CatalogJson.ToDictionary(fields));
    }

    private static CatalogModel? DecodeModel(string providerId, string key, JsonElement fields)
    {
        var id = NonEmptyString(key) ?? fields.NonEmpty("id");
        if (id is null)
        {
            return null;
        }

        var limitsElement = fields.Child("limit");
        var limits = new ModelLimits(
            limitsElement?.OptLong("context"),
            limitsElement?.OptLong("input"),
            limitsElement?.OptLong("output"));

        var costElement = fields.Child("cost");
        var pricing = new ModelPricing(
            costElement?.OptDouble("input"),
            costElement?.OptDouble("output"),
            costElement?.OptDouble("cache_read"),
            costElement?.OptDouble("cache_write"),
            costElement?.OptDouble("reasoning"),
            costElement?.OptDouble("input_audio"),
            costElement?.OptDouble("output_audio"));

        var inputModalities = fields.Child("modalities")?.StringList("input") ?? Array.Empty<string>();
        var outputModalities = fields.Child("modalities")?.StringList("output") ?? Array.Empty<string>();
        var attachment = fields.IsTrue("attachment");
        var efforts = new List<string>();
        if (fields.TryGetProperty("reasoning_options", out var options) && options.ValueKind == JsonValueKind.Array)
        {
            foreach (var option in options.EnumerateArray())
            {
                if (option.ValueKind != JsonValueKind.Object)
                {
                    continue;
                }
                efforts.AddRange(option.StringList("values"));
            }
        }

        var capabilities = new ModelCapabilities(
            fields.IsTrue("reasoning"),
            efforts,
            fields.IsTrue("tool_call"),
            fields.IsTrue("structured_output"),
            attachment,
            attachment || CatalogJson.HasPrefix(inputModalities, "image"),
            CatalogJson.HasPrefix(inputModalities, "audio"),
            CatalogJson.HasPrefix(outputModalities, "audio"),
            inputModalities,
            outputModalities,
            fields.IsTrue("open_weights"),
            fields.OptBool("temperature"));

        return new CatalogModel(
            id,
            providerId,
            fields.OptString("name") ?? id,
            fields.OptString("family"),
            fields.OptString("description"),
            fields.NonEmpty("release_date"),
            fields.NonEmpty("last_updated"),
            fields.NonEmpty("knowledge"),
            fields.NonEmpty("canonical_model_id"),
            ModelStatusKinds.FromRaw(fields.OptString("status")),
            limits,
            capabilities,
            pricing,
            CatalogJson.ToDictionary(fields));
    }

    private static string? NonEmptyString(string? value)
        => string.IsNullOrWhiteSpace(value) ? null : value;

    /// <summary>Parses an ISO-8601 instant or a date-only value (midnight UTC).</summary>
    public static DateTimeOffset? ParseTimestamp(string? raw)
    {
        if (string.IsNullOrWhiteSpace(raw))
        {
            return null;
        }

        if (DateTimeOffset.TryParse(
                raw,
                CultureInfo.InvariantCulture,
                DateTimeStyles.RoundtripKind | DateTimeStyles.AssumeUniversal,
                out var instant))
        {
            return instant;
        }

        if (DateTime.TryParseExact(
                raw,
                "yyyy-MM-dd",
                CultureInfo.InvariantCulture,
                DateTimeStyles.None,
                out var date))
        {
            return new DateTimeOffset(date, TimeSpan.Zero);
        }

        return null;
    }

    /// <summary>Formats like Java's <c>Instant.toString()</c> (ISO-8601 with Z).</summary>
    public static string? FormatTimestamp(DateTimeOffset? value)
        => value?.ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss'Z'", CultureInfo.InvariantCulture);
}
