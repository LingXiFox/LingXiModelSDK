namespace LingXiModelSDK.Catalog;

/// <summary>
/// Deterministic catalog ordering shared by every LingXi SDK: a natural sort
/// where digit runs compare numerically, plus featured-provider ranking.
/// </summary>
public static class CatalogOrdering
{
    private readonly record struct Token(bool IsNumber, string Raw, string Trimmed);

    private static List<Token> Tokenize(string? text)
    {
        var tokens = new List<Token>();
        if (string.IsNullOrEmpty(text))
        {
            return tokens;
        }
        var digits = new System.Text.StringBuilder();
        var words = new System.Text.StringBuilder();
        foreach (var ch in text)
        {
            if (ch is >= '0' and <= '9')
            {
                if (words.Length > 0)
                {
                    tokens.Add(new Token(false, words.ToString().ToLowerInvariant(), string.Empty));
                    words.Clear();
                }
                digits.Append(ch);
            }
            else
            {
                if (digits.Length > 0)
                {
                    tokens.Add(NumberToken(digits.ToString()));
                    digits.Clear();
                }
                words.Append(ch);
            }
        }
        if (digits.Length > 0)
        {
            tokens.Add(NumberToken(digits.ToString()));
        }
        if (words.Length > 0)
        {
            tokens.Add(new Token(false, words.ToString().ToLowerInvariant(), string.Empty));
        }
        return tokens;
    }

    private static Token NumberToken(string raw)
    {
        var trimmed = raw.TrimStart('0');
        return new Token(true, raw, trimmed.Length == 0 ? "0" : trimmed);
    }

    public static int CompareNatural(string? left, string? right)
    {
        var a = Tokenize(left);
        var b = Tokenize(right);
        var limit = Math.Min(a.Count, b.Count);
        for (var i = 0; i < limit; i++)
        {
            var lt = a[i];
            var rt = b[i];
            if (lt.IsNumber && !rt.IsNumber)
            {
                return -1;
            }
            if (!lt.IsNumber && rt.IsNumber)
            {
                return 1;
            }
            if (lt.IsNumber)
            {
                var byLength = lt.Trimmed.Length.CompareTo(rt.Trimmed.Length);
                if (byLength != 0)
                {
                    return byLength;
                }
                var byValue = string.CompareOrdinal(lt.Trimmed, rt.Trimmed);
                if (byValue != 0)
                {
                    return byValue;
                }
                var byRaw = string.CompareOrdinal(lt.Raw, rt.Raw);
                if (byRaw != 0)
                {
                    return byRaw;
                }
            }
            else
            {
                var byText = string.CompareOrdinal(lt.Raw, rt.Raw);
                if (byText != 0)
                {
                    return byText;
                }
            }
        }
        return a.Count.CompareTo(b.Count);
    }

    /// <summary>Sorts providers featured-first, then by natural name, then by id.</summary>
    public static IReadOnlyList<CatalogProvider> SortProviders(
        IReadOnlyList<CatalogProvider> providers,
        IReadOnlyList<string>? featured = null)
    {
        var ranks = new Dictionary<string, int>(StringComparer.Ordinal);
        if (featured is not null)
        {
            for (var index = 0; index < featured.Count; index++)
            {
                ranks[featured[index].Trim().ToLowerInvariant()] = index;
            }
        }
        return providers
            .OrderBy(provider => ranks.TryGetValue(provider.Id.Trim().ToLowerInvariant(), out var rank) ? rank : int.MaxValue)
            .ThenBy(provider => provider, Comparer<CatalogProvider>.Create((a, b) =>
            {
                var byName = CompareNatural(a.Name, b.Name);
                return byName != 0 ? byName : string.CompareOrdinal(a.Id, b.Id);
            }))
            .ToArray();
    }

    /// <summary>Sorts models newest-release first, then by natural family and name, then id.</summary>
    public static IReadOnlyList<CatalogModel> ModelsInStableOrder(IEnumerable<CatalogModel> models)
        => models.ToArray().OrderBy(model => model, Comparer<CatalogModel>.Create(CompareModels)).ToArray();

    private static int CompareModels(CatalogModel a, CatalogModel b)
    {
        var left = a.ReleaseDate ?? string.Empty;
        var right = b.ReleaseDate ?? string.Empty;
        if (!string.Equals(left, right, StringComparison.Ordinal))
        {
            if (left.Length == 0)
            {
                return 1;
            }
            if (right.Length == 0)
            {
                return -1;
            }
            var byDate = string.CompareOrdinal(right, left);
            if (byDate != 0)
            {
                return byDate;
            }
        }
        var byFamily = CompareNatural(a.Family, b.Family);
        if (byFamily != 0)
        {
            return byFamily;
        }
        var byName = CompareNatural(a.Name, b.Name);
        if (byName != 0)
        {
            return byName;
        }
        return string.CompareOrdinal(a.Id, b.Id);
    }
}
