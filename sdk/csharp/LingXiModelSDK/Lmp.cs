using System.Text.Json;
using System.Text.Json.Nodes;
using LingXiModelSDK.Core;

namespace LingXiModelSDK.Lmp;

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

public sealed record LmpMessage(string Role, IReadOnlyList<LmpContentBlock> Content)
{
    public static LmpMessage User(string text) => new("user", new[] { new LmpTextBlock(text) });

    public static LmpMessage Assistant(string text) => new("assistant", new[] { new LmpTextBlock(text) });

    public static LmpMessage System(string text) => new("system", new[] { new LmpTextBlock(text) });

    public JsonObject ToJson()
    {
        var content = new JsonArray();
        foreach (var block in Content)
        {
            content.Add(block.ToJson());
        }
        return new JsonObject { ["role"] = Role, ["content"] = content };
    }
}

public sealed record LmpToolDefinition(string Name, JsonElement Parameters, string? Description = null)
{
    public JsonObject ToJson()
    {
        var obj = new JsonObject { ["name"] = Name, ["parameters"] = JsonNode.Parse(Parameters.GetRawText()) };
        if (Description is not null)
        {
            obj["description"] = Description;
        }
        return obj;
    }
}

/// <summary>LMP tool selection: <c>auto</c>, <c>none</c>, <c>required</c> or one named tool.</summary>
public sealed record LmpToolChoice(string Kind, string? ToolName = null)
{
    public static readonly LmpToolChoice Auto = new("auto");
    public static readonly LmpToolChoice None = new("none");
    public static readonly LmpToolChoice Required = new("required");

    public static LmpToolChoice Tool(string name) => new("tool", name);

    public JsonNode ToJson() => Kind switch
    {
        "tool" => new JsonObject { ["tool"] = ToolName ?? throw new InvalidOperationException("tool choice needs a name") },
        "auto" or "none" or "required" => JsonValue.Create(Kind)!,
        _ => throw new InvalidOperationException($"unknown tool choice '{Kind}'"),
    };
}

public sealed record LmpThinkingConfig(string? Effort = null, long? BudgetTokens = null)
{
    public JsonObject ToJson()
    {
        var obj = new JsonObject();
        if (Effort is not null)
        {
            obj["effort"] = Effort;
        }
        if (BudgetTokens is not null)
        {
            obj["budgetTokens"] = BudgetTokens.Value;
        }
        return obj;
    }
}

public sealed record LmpResponseFormat(string Kind, string? Name = null, JsonElement? Schema = null, bool? Strict = null)
{
    public static readonly LmpResponseFormat Text = new("text");
    public static readonly LmpResponseFormat JsonObject = new("json_object");

    public static LmpResponseFormat JsonSchema(string name, JsonElement schema, bool? strict = null)
        => new("json_schema", name, schema, strict);

    public JsonObject ToJson()
    {
        var obj = new JsonObject { ["type"] = Kind };
        if (Kind != "json_schema")
        {
            return obj;
        }
        var schema = new JsonObject { ["name"] = Name };
        if (Schema is { } value)
        {
            schema["schema"] = JsonNode.Parse(value.GetRawText());
        }
        if (Strict is { } strict)
        {
            schema["strict"] = strict;
        }
        obj["schema"] = schema;
        return obj;
    }
}

/// <summary>A LingXi Model Protocol (LMP) request, translated by the Rust core.</summary>
public sealed record LmpRequest
{
    public required string Model { get; init; }

    public required IReadOnlyList<LmpMessage> Messages { get; init; }

    public long? MaxTokens { get; init; }

    public double? Temperature { get; init; }

    public IReadOnlyList<LmpToolDefinition>? Tools { get; init; }

    public LmpToolChoice? ToolChoice { get; init; }

    public LmpThinkingConfig? Thinking { get; init; }

    public LmpResponseFormat? ResponseFormat { get; init; }

    public long? TimeoutMs { get; init; }

    public IReadOnlyDictionary<string, JsonElement>? ProviderExtensions { get; init; }

    /// <summary>Minimal single-prompt request, the shape every provider can express.</summary>
    public static LmpRequest Text(string model, string prompt) => new()
    {
        Model = model,
        Messages = new[] { LmpMessage.User(prompt) },
    };

    public string ToJson() => ToJsonObject().ToJsonString();

    public JsonObject ToJsonObject()
    {
        var messages = new JsonArray();
        foreach (var message in Messages)
        {
            messages.Add(message.ToJson());
        }
        var obj = new JsonObject { ["model"] = Model, ["messages"] = messages };
        if (MaxTokens is { } maxTokens)
        {
            obj["maxTokens"] = maxTokens;
        }
        if (Temperature is { } temperature)
        {
            obj["temperature"] = temperature;
        }
        if (Tools is { Count: > 0 } tools)
        {
            var array = new JsonArray();
            foreach (var tool in tools)
            {
                array.Add(tool.ToJson());
            }
            obj["tools"] = array;
        }
        if (ToolChoice is { } toolChoice)
        {
            obj["toolChoice"] = toolChoice.ToJson();
        }
        if (Thinking is { } thinking)
        {
            obj["thinking"] = thinking.ToJson();
        }
        if (ResponseFormat is { } format)
        {
            obj["responseFormat"] = format.ToJson();
        }
        if (TimeoutMs is { } timeoutMs)
        {
            obj["timeoutMs"] = timeoutMs;
        }
        if (ProviderExtensions is { Count: > 0 } extensions)
        {
            var ext = new JsonObject();
            foreach (var (key, value) in extensions)
            {
                ext[key] = JsonNode.Parse(value.GetRawText());
            }
            obj["providerExtensions"] = ext;
        }
        return obj;
    }
}

// ---------------------------------------------------------------------------
// Content blocks
// ---------------------------------------------------------------------------

/// <summary>One typed LMP content block; the wire form is tagged by <c>type</c>.</summary>
public abstract record LmpContentBlock
{
    public abstract string Type { get; }

    public abstract JsonObject ToJson();

    public static LmpContentBlock Parse(JsonElement element)
    {
        var type = element.OptString("type") ?? "unknown";
        return type switch
        {
            "text" => new LmpTextBlock(element.GetProperty("text").GetString() ?? string.Empty, element.OptString("signature")),
            "thinking" => new LmpThinkingBlock(element.GetProperty("thinking").GetString() ?? string.Empty, element.OptString("signature")),
            "image_url" => new LmpImageUrlBlock(element.GetProperty("url").GetString() ?? string.Empty),
            "image_base64" => new LmpImageBase64Block(element.OptString("mediaType") ?? string.Empty, element.OptString("data") ?? string.Empty),
            "audio" => new LmpAudioBlock(element.OptString("mediaType") ?? string.Empty, element.OptString("data") ?? string.Empty),
            "tool_call" => new LmpToolCallBlock(
                element.OptString("id") ?? string.Empty,
                element.OptString("name") ?? string.Empty,
                element.TryGetProperty("arguments", out var arguments) ? arguments.Clone() : default,
                element.OptString("signature")),
            "tool_result" => new LmpToolResultBlock(
                element.OptString("toolCallId") ?? string.Empty,
                element.OptString("content") ?? string.Empty,
                element.OptBool("isError") ?? false),
            _ => new LmpUnknownContentBlock(type, element.Clone()),
        };
    }
}

public sealed record LmpTextBlock(string Text, string? Signature = null) : LmpContentBlock
{
    public override string Type => "text";

    public override JsonObject ToJson()
    {
        var obj = new JsonObject { ["type"] = Type, ["text"] = Text };
        if (Signature is not null)
        {
            obj["signature"] = Signature;
        }
        return obj;
    }
}

public sealed record LmpThinkingBlock(string Thinking, string? Signature = null) : LmpContentBlock
{
    public override string Type => "thinking";

    public override JsonObject ToJson()
    {
        var obj = new JsonObject { ["type"] = Type, ["thinking"] = Thinking };
        if (Signature is not null)
        {
            obj["signature"] = Signature;
        }
        return obj;
    }
}

public sealed record LmpImageUrlBlock(string Url) : LmpContentBlock
{
    public override string Type => "image_url";

    public override JsonObject ToJson() => new() { ["type"] = Type, ["url"] = Url };
}

public sealed record LmpImageBase64Block(string MediaType, string Data) : LmpContentBlock
{
    public override string Type => "image_base64";

    public override JsonObject ToJson() => new() { ["type"] = Type, ["mediaType"] = MediaType, ["data"] = Data };
}

public sealed record LmpAudioBlock(string MediaType, string Data) : LmpContentBlock
{
    public override string Type => "audio";

    public override JsonObject ToJson() => new() { ["type"] = Type, ["mediaType"] = MediaType, ["data"] = Data };
}

public sealed record LmpToolCallBlock(string Id, string Name, JsonElement Arguments, string? Signature = null) : LmpContentBlock
{
    public override string Type => "tool_call";

    public override JsonObject ToJson()
    {
        var obj = new JsonObject
        {
            ["type"] = Type,
            ["id"] = Id,
            ["name"] = Name,
            ["arguments"] = Arguments.ValueKind == JsonValueKind.Undefined
                ? new JsonObject()
                : JsonNode.Parse(Arguments.GetRawText()),
        };
        if (Signature is not null)
        {
            obj["signature"] = Signature;
        }
        return obj;
    }
}

public sealed record LmpToolResultBlock(string ToolCallId, string Content, bool IsError = false) : LmpContentBlock
{
    public override string Type => "tool_result";

    public override JsonObject ToJson() => new()
    {
        ["type"] = Type,
        ["toolCallId"] = ToolCallId,
        ["content"] = Content,
        ["isError"] = IsError,
    };
}

/// <summary>Forward-compatible carrier for block types this SDK build does not model.</summary>
public sealed record LmpUnknownContentBlock(string UnknownType, JsonElement Raw) : LmpContentBlock
{
    public override string Type => UnknownType;

    public override JsonObject ToJson() => (JsonObject)JsonNode.Parse(Raw.GetRawText())!;
}

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// <summary>Token accounting for one LMP response or stream event.</summary>
public sealed record LmpUsage(
    long InputTokens,
    long OutputTokens,
    long TotalTokens,
    long? ReasoningTokens = null,
    long? CacheReadTokens = null,
    long? CacheWriteTokens = null)
{
    public static LmpUsage FromJson(JsonElement element) => new(
        element.OptLong("inputTokens") ?? 0,
        element.OptLong("outputTokens") ?? 0,
        element.OptLong("totalTokens") ?? 0,
        element.OptLong("reasoningTokens"),
        element.OptLong("cacheReadTokens"),
        element.OptLong("cacheWriteTokens"));
}

/// <summary>A normalized LingXi Model Protocol response.</summary>
public sealed record LmpResponse(
    string? Id,
    string? Model,
    IReadOnlyList<LmpContentBlock> Content,
    string? FinishReason,
    LmpUsage Usage,
    JsonElement Raw)
{
    public static LmpResponse FromJson(JsonElement element)
    {
        var content = new List<LmpContentBlock>();
        if (element.TryGetProperty("content", out var blocks) && blocks.ValueKind == JsonValueKind.Array)
        {
            foreach (var block in blocks.EnumerateArray())
            {
                content.Add(LmpContentBlock.Parse(block));
            }
        }
        var usage = element.TryGetProperty("usage", out var usageElement) && usageElement.ValueKind == JsonValueKind.Object
            ? LmpUsage.FromJson(usageElement)
            : new LmpUsage(0, 0, 0);
        return new LmpResponse(
            element.OptString("id"),
            element.OptString("model"),
            content,
            element.OptString("finishReason"),
            usage,
            element.Clone());
    }

    /// <summary>Concatenated text blocks, the assistant's visible answer.</summary>
    public string Text => string.Concat(Content.OfType<LmpTextBlock>().Select(b => b.Text));

    /// <summary>Concatenated thinking blocks.</summary>
    public string Thinking => string.Concat(Content.OfType<LmpThinkingBlock>().Select(b => b.Thinking));
}

// ---------------------------------------------------------------------------
// Stream events
// ---------------------------------------------------------------------------

public enum LmpStreamEventKind
{
    MessageStart,
    ThinkingDelta,
    TextDelta,
    ToolCallDelta,
    Usage,
    MessageEnd,
    Error,
    Unknown,
}

/// <summary>One LMP stream event; the wire form is tagged by <c>type</c>.</summary>
public sealed record LmpStreamEvent
{
    public required LmpStreamEventKind Kind { get; init; }

    public required string Type { get; init; }

    public string? Id { get; init; }

    public string? Delta { get; init; }

    public string? Signature { get; init; }

    public int? Index { get; init; }

    public string? ToolCallId { get; init; }

    public string? Name { get; init; }

    public string? ArgumentsDelta { get; init; }

    public LmpUsage? Usage { get; init; }

    public string? FinishReason { get; init; }

    public string? Code { get; init; }

    public string? Message { get; init; }

    public required JsonElement Raw { get; init; }

    public static LmpStreamEvent FromJson(JsonElement element)
    {
        var type = element.OptString("type") ?? "unknown";
        var kind = type switch
        {
            "message_start" => LmpStreamEventKind.MessageStart,
            "thinking_delta" => LmpStreamEventKind.ThinkingDelta,
            "text_delta" => LmpStreamEventKind.TextDelta,
            "tool_call_delta" => LmpStreamEventKind.ToolCallDelta,
            "usage" => LmpStreamEventKind.Usage,
            "message_end" => LmpStreamEventKind.MessageEnd,
            "error" => LmpStreamEventKind.Error,
            _ => LmpStreamEventKind.Unknown,
        };
        return new LmpStreamEvent
        {
            Kind = kind,
            Type = type,
            Id = element.OptString("id"),
            Delta = element.OptString("delta"),
            Signature = element.OptString("signature"),
            Index = (int?)element.OptLong("index"),
            ToolCallId = element.OptString("id"),
            Name = element.OptString("name"),
            ArgumentsDelta = element.OptString("argumentsDelta"),
            Usage = element.TryGetProperty("usage", out var usage) && usage.ValueKind == JsonValueKind.Object
                ? LmpUsage.FromJson(usage)
                : null,
            FinishReason = element.OptString("finishReason"),
            Code = element.OptString("code"),
            Message = element.OptString("message"),
            Raw = element.Clone(),
        };
    }
}

// ---------------------------------------------------------------------------
// Embeddings and reranking
// ---------------------------------------------------------------------------

public sealed record LmpEmbeddingResponse(
    IReadOnlyList<IReadOnlyList<double>> Embeddings,
    LmpUsage Usage,
    JsonElement Raw)
{
    public static LmpEmbeddingResponse FromJson(JsonElement element)
    {
        var vectors = new List<IReadOnlyList<double>>();
        if (element.TryGetProperty("embeddings", out var embeddings) && embeddings.ValueKind == JsonValueKind.Array)
        {
            foreach (var vector in embeddings.EnumerateArray())
            {
                if (vector.ValueKind != JsonValueKind.Array)
                {
                    continue;
                }
                vectors.Add(vector.EnumerateArray().Select(v => v.GetDouble()).ToArray());
            }
        }
        var usage = element.TryGetProperty("usage", out var usageElement) && usageElement.ValueKind == JsonValueKind.Object
            ? LmpUsage.FromJson(usageElement)
            : new LmpUsage(0, 0, 0);
        return new LmpEmbeddingResponse(vectors, usage, element.Clone());
    }
}

public sealed record LmpRerankResult(int Index, double RelevanceScore);

public sealed record LmpRerankResponse(IReadOnlyList<LmpRerankResult> Results, JsonElement Raw)
{
    public static LmpRerankResponse FromJson(JsonElement element)
    {
        var results = new List<LmpRerankResult>();
        if (element.TryGetProperty("results", out var items) && items.ValueKind == JsonValueKind.Array)
        {
            foreach (var item in items.EnumerateArray())
            {
                results.Add(new LmpRerankResult(
                    (int)(item.OptLong("index") ?? 0),
                    item.OptDouble("relevanceScore") ?? 0));
            }
        }
        return new LmpRerankResponse(results, element.Clone());
    }
}
