using System.Text.Json;

namespace LingXiModelSDK.Catalog;

/// <summary>Lossless <see cref="JsonElement"/> to CLR object conversion for preserved fields.</summary>
internal static class CatalogJson
{
    public static object? ToObject(JsonElement element) => element.ValueKind switch
    {
        JsonValueKind.Object => element.EnumerateObject().ToDictionary(p => p.Name, p => ToObject(p.Value), StringComparer.Ordinal),
        JsonValueKind.Array => element.EnumerateArray().Select(ToObject).ToArray(),
        JsonValueKind.String => element.GetString(),
        JsonValueKind.Number => element.TryGetInt64(out var integer) ? integer : element.GetDouble(),
        JsonValueKind.True => true,
        JsonValueKind.False => false,
        _ => null,
    };

    public static IReadOnlyDictionary<string, object?> ToDictionary(JsonElement element)
        => element.ValueKind == JsonValueKind.Object
            ? element.EnumerateObject().ToDictionary(p => p.Name, p => ToObject(p.Value), StringComparer.Ordinal)
            : new Dictionary<string, object?>(StringComparer.Ordinal);

    public static string? OptString(this JsonElement element, string name)
        => element.ValueKind == JsonValueKind.Object
            && element.TryGetProperty(name, out var value)
            && value.ValueKind == JsonValueKind.String
                ? value.GetString()
                : null;

    public static string? NonEmpty(this JsonElement element, string name)
    {
        var text = element.OptString(name);
        return string.IsNullOrWhiteSpace(text) ? null : text;
    }

    public static long? OptLong(this JsonElement element, string name)
    {
        if (element.ValueKind != JsonValueKind.Object || !element.TryGetProperty(name, out var value))
        {
            return null;
        }
        if (value.ValueKind != JsonValueKind.Number)
        {
            return null;
        }
        if (value.TryGetInt64(out var integer))
        {
            return integer;
        }
        var number = value.GetDouble();
        return Math.Round(number) == number ? (long)number : null;
    }

    public static double? OptDouble(this JsonElement element, string name)
        => element.ValueKind == JsonValueKind.Object
            && element.TryGetProperty(name, out var value)
            && value.ValueKind == JsonValueKind.Number
                ? value.GetDouble()
                : null;

    public static bool? OptBool(this JsonElement element, string name)
    {
        if (element.ValueKind != JsonValueKind.Object || !element.TryGetProperty(name, out var value))
        {
            return null;
        }
        return value.ValueKind switch
        {
            JsonValueKind.True => true,
            JsonValueKind.False => false,
            _ => null,
        };
    }

    public static bool IsTrue(this JsonElement element, string name) => element.OptBool(name) == true;

    public static IReadOnlyList<string> StringList(this JsonElement element, string name)
        => element.ValueKind == JsonValueKind.Object
            && element.TryGetProperty(name, out var value)
            && value.ValueKind == JsonValueKind.Array
                ? value.EnumerateArray()
                    .Where(item => item.ValueKind == JsonValueKind.String)
                    .Select(item => item.GetString()!)
                    .ToArray()
                : Array.Empty<string>();

    public static JsonElement? Child(this JsonElement element, string name)
        => element.ValueKind == JsonValueKind.Object
            && element.TryGetProperty(name, out var value)
            && value.ValueKind == JsonValueKind.Object
                ? value
                : null;

    public static bool HasPrefix(IReadOnlyList<string> values, string prefix)
        => values.Any(item => item.ToLowerInvariant().StartsWith(prefix, StringComparison.Ordinal));
}
