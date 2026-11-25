import Foundation

/// LingXi Model Protocol (LMP) value types shared by the Unified provider.
///
/// The wire shape is owned by the Rust core: `LmpRequest` is encoded to JSON,
/// the core translates it to a provider payload, and the core normalizes the
/// provider answer back into the shapes decoded here.

public enum LmpContentBlock: Sendable, Equatable {
    case text(String, signature: String? = nil)
    case thinking(String, signature: String?)
    case imageURL(String)
    case imageBase64(mediaType: String, data: String)
    case audio(mediaType: String, data: String)
    case toolCall(id: String, name: String, argumentsJSON: String, signature: String? = nil)
    case toolResult(toolCallID: String, content: String, isError: Bool)
}

extension LmpContentBlock {
    func json() -> [String: Any] {
        switch self {
        case .text(let text, let signature):
            var out: [String: Any] = ["type": "text", "text": text]
            if let signature, !signature.isEmpty { out["signature"] = signature }
            return out
        case .thinking(let thinking, let signature):
            var out: [String: Any] = ["type": "thinking", "thinking": thinking]
            if let signature, !signature.isEmpty { out["signature"] = signature }
            return out
        case .imageURL(let url):
            return ["type": "image_url", "url": url]
        case .imageBase64(let mediaType, let data):
            return ["type": "image_base64", "mediaType": mediaType, "data": data]
        case .audio(let mediaType, let data):
            return ["type": "audio", "mediaType": mediaType, "data": data]
        case .toolCall(let id, let name, let argumentsJSON, let signature):
            var out: [String: Any] = ["type": "tool_call", "id": id, "name": name]
            out["arguments"] = (try? JSONSerialization.jsonObject(with: Data(argumentsJSON.utf8))) ?? [:]
            if let signature, !signature.isEmpty { out["signature"] = signature }
            return out
        case .toolResult(let toolCallID, let content, let isError):
            return ["type": "tool_result", "toolCallId": toolCallID, "content": content, "isError": isError]
        }
    }

    static func decode(_ json: [String: Any]) -> LmpContentBlock? {
        switch json["type"] as? String {
        case "text":
            guard let text = json["text"] as? String else { return nil }
            return .text(text, signature: json["signature"] as? String)
        case "thinking":
            guard let thinking = json["thinking"] as? String else { return nil }
            return .thinking(thinking, signature: json["signature"] as? String)
        case "image_url":
            guard let url = json["url"] as? String else { return nil }
            return .imageURL(url)
        case "image_base64":
            guard let media = json["mediaType"] as? String, let data = json["data"] as? String else { return nil }
            return .imageBase64(mediaType: media, data: data)
        case "audio":
            guard let media = json["mediaType"] as? String, let data = json["data"] as? String else { return nil }
            return .audio(mediaType: media, data: data)
        case "tool_call":
            guard let id = json["id"] as? String, let name = json["name"] as? String else { return nil }
            let arguments = json["arguments"] ?? [:]
            let encoded: String
            if let data = try? JSONSerialization.data(withJSONObject: arguments, options: [.sortedKeys]),
               let text = String(data: data, encoding: .utf8) {
                encoded = text
            } else {
                encoded = "{}"
            }
            return .toolCall(id: id, name: name, argumentsJSON: encoded, signature: json["signature"] as? String)
        case "tool_result":
            let id = (json["toolCallId"] as? String) ?? (json["tool_call_id"] as? String) ?? ""
            let content = json["content"] as? String ?? ""
            let isError = (json["isError"] as? Bool) ?? (json["is_error"] as? Bool) ?? false
            return .toolResult(toolCallID: id, content: content, isError: isError)
        default:
            return nil
        }
    }

    public var textValue: String? {
        if case .text(let text, _) = self { return text }
        return nil
    }
}

public struct LmpMessage: Sendable {
    public let role: String
    public let content: [LmpContentBlock]

    public init(role: String, content: [LmpContentBlock]) {
        self.role = role
        self.content = content
    }

    public init(role: String, text: String) {
        self.init(role: role, content: [.text(text)])
    }

    func json() -> [String: Any] {
        ["role": role, "content": content.map { $0.json() }]
    }
}

/// Arbitrary JSON payload (tool schemas, provider extensions).
///
/// Foundation's `Any` is not Sendable, so the value is boxed and the
/// compiler-safety guarantee is carried by the immutability of the box.
public struct LmpJSON: @unchecked Sendable {
    public let raw: Any

    public init(_ raw: Any) { self.raw = raw }

    public init?(json text: String) {
        guard let data = text.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) else { return nil }
        self.raw = object
    }
}

public struct LmpToolDefinition: Sendable {
    public let name: String
    public let description: String?
    public let parameters: LmpJSON

    public init(name: String, description: String? = nil, parameters: LmpJSON) {
        self.name = name
        self.description = description
        self.parameters = parameters
    }

    func json() -> [String: Any] {
        var out: [String: Any] = ["name": name, "parameters": parameters.raw]
        if let description { out["description"] = description }
        return out
    }
}

public enum LmpToolChoice: Sendable, Equatable {
    case auto
    case none
    case required
    case tool(String)

    var json: Any {
        switch self {
        case .auto: return "auto"
        case .none: return "none"
        case .required: return "required"
        case .tool(let name): return name
        }
    }
}

public struct LmpThinkingConfig: Sendable {
    public let effort: String?
    public let budgetTokens: Int?

    public init(effort: String? = nil, budgetTokens: Int? = nil) {
        self.effort = effort
        self.budgetTokens = budgetTokens
    }

    func json() -> [String: Any] {
        var out: [String: Any] = [:]
        if let effort { out["effort"] = effort }
        if let budgetTokens { out["budgetTokens"] = budgetTokens }
        return out
    }
}

public enum LmpResponseFormat: Sendable {
    case text
    case jsonObject
    case jsonSchema(name: String, schema: LmpJSON, strict: Bool)

    var json: [String: Any] {
        switch self {
        case .text: return ["type": "text"]
        case .jsonObject: return ["type": "json_object"]
        case .jsonSchema(let name, let schema, let strict):
            return ["type": "json_schema", "name": name, "schema": schema.raw, "strict": strict]
        }
    }
}

public struct LmpRequest: Sendable {
    public let model: String
    public let messages: [LmpMessage]
    public let maxTokens: Int?
    public let temperature: Double?
    public let tools: [LmpToolDefinition]
    public let toolChoice: LmpToolChoice?
    public let thinking: LmpThinkingConfig?
    public let responseFormat: LmpResponseFormat?
    public let timeoutMilliseconds: Int?
    public let providerExtensions: LmpJSON?

    public init(
        model: String,
        messages: [LmpMessage],
        maxTokens: Int? = nil,
        temperature: Double? = nil,
        tools: [LmpToolDefinition] = [],
        toolChoice: LmpToolChoice? = nil,
        thinking: LmpThinkingConfig? = nil,
        responseFormat: LmpResponseFormat? = nil,
        timeoutMilliseconds: Int? = nil,
        providerExtensions: LmpJSON? = nil
    ) {
        self.model = model
        self.messages = messages
        self.maxTokens = maxTokens
        self.temperature = temperature
        self.tools = tools
        self.toolChoice = toolChoice
        self.thinking = thinking
        self.responseFormat = responseFormat
        self.timeoutMilliseconds = timeoutMilliseconds
        self.providerExtensions = providerExtensions
    }

    public func json() -> [String: Any] {
        var out: [String: Any] = ["model": model, "messages": messages.map { $0.json() }]
        if let maxTokens { out["maxTokens"] = maxTokens }
        if let temperature { out["temperature"] = temperature }
        if !tools.isEmpty { out["tools"] = tools.map { $0.json() } }
        if let toolChoice { out["toolChoice"] = toolChoice.json }
        if let thinking { out["thinking"] = thinking.json() }
        if let responseFormat { out["responseFormat"] = responseFormat.json }
        if let timeoutMilliseconds { out["timeoutMs"] = timeoutMilliseconds }
        if let providerExtensions { out["providerExtensions"] = providerExtensions.raw }
        return out
    }
}

public struct LmpUsage: Sendable, Equatable {
    public let inputTokens: Int
    public let outputTokens: Int
    public let totalTokens: Int
    public let reasoningTokens: Int?
    public let cacheReadTokens: Int?
    public let cacheWriteTokens: Int?

    public init(
        inputTokens: Int,
        outputTokens: Int,
        totalTokens: Int,
        reasoningTokens: Int? = nil,
        cacheReadTokens: Int? = nil,
        cacheWriteTokens: Int? = nil
    ) {
        self.inputTokens = inputTokens
        self.outputTokens = outputTokens
        self.totalTokens = totalTokens
        self.reasoningTokens = reasoningTokens
        self.cacheReadTokens = cacheReadTokens
        self.cacheWriteTokens = cacheWriteTokens
    }

    static func decode(_ json: [String: Any]?) -> LmpUsage {
        let value = json ?? [:]
        func int(_ key: String) -> Int { (value[key] as? NSNumber)?.intValue ?? 0 }
        func optional(_ key: String) -> Int? { (value[key] as? NSNumber)?.intValue }
        return LmpUsage(
            inputTokens: int("inputTokens"),
            outputTokens: int("outputTokens"),
            totalTokens: int("totalTokens"),
            reasoningTokens: optional("reasoningTokens"),
            cacheReadTokens: optional("cacheReadTokens"),
            cacheWriteTokens: optional("cacheWriteTokens")
        )
    }
}

public struct LmpResponse: Sendable {
    public let id: String?
    public let model: String?
    public let content: [LmpContentBlock]
    public let finishReason: String?
    public let usage: LmpUsage

    public init(id: String?, model: String?, content: [LmpContentBlock], finishReason: String?, usage: LmpUsage) {
        self.id = id
        self.model = model
        self.content = content
        self.finishReason = finishReason
        self.usage = usage
    }

    public var text: String {
        content.compactMap { $0.textValue }.joined()
    }

    public var thinking: String {
        content.compactMap { block in
            if case .thinking(let thinking, _) = block { return thinking }
            return nil
        }.joined()
    }

    public var toolCalls: [(id: String, name: String, argumentsJSON: String)] {
        content.compactMap { block in
            if case .toolCall(let id, let name, let argumentsJSON, _) = block { return (id, name, argumentsJSON) }
            return nil
        }
    }

    static func decode(_ json: [String: Any]) throws -> LmpResponse {
        let blocks = (json["content"] as? [[String: Any]] ?? []).compactMap { LmpContentBlock.decode($0) }
        return LmpResponse(
            id: json["id"] as? String,
            model: json["model"] as? String,
            content: blocks,
            finishReason: json["finishReason"] as? String,
            usage: LmpUsage.decode(json["usage"] as? [String: Any])
        )
    }
}

/// One normalized streaming event. Tags are snake_case; fields are camelCase.
public enum LmpStreamEvent: Sendable, Equatable {
    case messageStart(id: String?)
    case thinkingDelta(delta: String, signature: String?)
    case textDelta(delta: String, signature: String?)
    case toolCallDelta(index: Int, id: String?, name: String?, argumentsDelta: String, signature: String?)
    case usage(LmpUsage)
    case messageEnd(finishReason: String?)
    case error(code: String, message: String)

    public static func decode(_ json: [String: Any]) -> LmpStreamEvent? {
        switch json["type"] as? String {
        case "message_start":
            return .messageStart(id: json["id"] as? String)
        case "thinking_delta":
            return .thinkingDelta(delta: json["delta"] as? String ?? "", signature: json["signature"] as? String)
        case "text_delta":
            return .textDelta(delta: json["delta"] as? String ?? "", signature: json["signature"] as? String)
        case "tool_call_delta":
            let index = (json["index"] as? NSNumber)?.intValue ?? 0
            return .toolCallDelta(
                index: index,
                id: json["id"] as? String,
                name: json["name"] as? String,
                argumentsDelta: json["argumentsDelta"] as? String ?? "",
                signature: json["signature"] as? String
            )
        case "usage":
            return .usage(LmpUsage.decode(json["usage"] as? [String: Any]))
        case "message_end":
            return .messageEnd(finishReason: json["finishReason"] as? String)
        case "error":
            return .error(code: json["code"] as? String ?? "provider_error", message: json["message"] as? String ?? "")
        default:
            return nil
        }
    }

    public var textDelta: String? {
        if case .textDelta(let delta, _) = self { return delta }
        return nil
    }

    public var finishReason: String? {
        if case .messageEnd(let reason) = self { return reason }
        return nil
    }
}

public struct LmpEmbeddingRequest: Sendable {
    public let model: String
    public let input: [String]

    public init(model: String, input: [String]) {
        self.model = model
        self.input = input
    }
}

public struct LmpEmbeddingResponse: Sendable {
    public let embeddings: [[Double]]
    public let usage: LmpUsage

    public init(embeddings: [[Double]], usage: LmpUsage) {
        self.embeddings = embeddings
        self.usage = usage
    }

    static func decode(_ json: [String: Any]) -> LmpEmbeddingResponse {
        let vectors = (json["embeddings"] as? [[Any]] ?? []).map { row in
            row.map { ($0 as? NSNumber)?.doubleValue ?? 0 }
        }
        return LmpEmbeddingResponse(embeddings: vectors, usage: LmpUsage.decode(json["usage"] as? [String: Any]))
    }
}

public struct LmpRerankResult: Sendable, Equatable {
    public let index: Int
    public let relevanceScore: Double

    public init(index: Int, relevanceScore: Double) {
        self.index = index
        self.relevanceScore = relevanceScore
    }
}

public struct LmpRerankResponse: Sendable {
    public let results: [LmpRerankResult]

    public init(results: [LmpRerankResult]) {
        self.results = results
    }

    static func decode(_ json: [String: Any]) -> LmpRerankResponse {
        let results = (json["results"] as? [[String: Any]] ?? []).map {
            LmpRerankResult(
                index: ($0["index"] as? NSNumber)?.intValue ?? 0,
                relevanceScore: ($0["relevanceScore"] as? NSNumber)?.doubleValue ?? 0
            )
        }
        return LmpRerankResponse(results: results)
    }
}

public enum LmpError: Error, LocalizedError, Sendable {
    case unsupportedFeature(protocol: String, feature: String)
    case auth(String)
    case rateLimit(String)
    case invalidRequest(String)
    case providerError(String)
    case timeout(String)
    case cancelled(String)
    case transport(String)
    case decode(String)

    public var errorDescription: String? {
        switch self {
        case .unsupportedFeature(let proto, let feature):
            return "Unsupported feature '\(feature)' for protocol '\(proto)'"
        case .auth(let m): return "Authentication error: \(m)"
        case .rateLimit(let m): return "Rate limited: \(m)"
        case .invalidRequest(let m): return "Invalid request: \(m)"
        case .providerError(let m): return "Provider error: \(m)"
        case .timeout(let m): return "Timed out: \(m)"
        case .cancelled(let m): return "Cancelled: \(m)"
        case .transport(let m): return "Transport error: \(m)"
        case .decode(let m): return "Decode error: \(m)"
        }
    }

    /// Maps a core error code onto the typed LMP error.
    public static func fromCoreCode(_ code: String, _ message: String, protocol proto: String) -> LmpError {
        switch code {
        case "unsupported_feature":
            return .unsupportedFeature(protocol: proto, feature: Self.featureName(message))
        case "auth": return .auth(message)
        case "rate_limit": return .rateLimit(message)
        case "invalid_request": return .invalidRequest(message)
        case "timeout": return .timeout(message)
        case "cancelled": return .cancelled(message)
        case "transport": return .transport(message)
        case "decode", "stream_parse": return .decode(message)
        default: return .providerError(message)
        }
    }

    /// Extracts `feature` from "unsupported feature 'x' for protocol 'y'".
    static func featureName(_ message: String) -> String {
        guard let range = message.range(of: "'") else { return message }
        let rest = message[range.upperBound...]
        guard let end = rest.firstIndex(of: "'") else { return String(rest) }
        return String(rest[..<end])
    }
}
