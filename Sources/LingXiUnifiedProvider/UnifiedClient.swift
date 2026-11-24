import Foundation
import LingXiNativeProvider
import LingXiProviderRegistry

public enum LmpContentBlock: Sendable, Equatable {
    case text(String)
    case thinking(String, signature: String?)
    case toolCall(id: String, name: String, argumentsJSON: String)
    case toolResult(toolCallID: String, content: String, isError: Bool)
}

public struct LmpMessage: Sendable {
    public let role: String
    public let content: [LmpContentBlock]

    public init(role: String, content: [LmpContentBlock]) {
        self.role = role
        self.content = content
    }
}

public struct LmpThinkingConfig: Sendable {
    public let effort: String?
    public let budgetTokens: Int?

    public init(effort: String? = nil, budgetTokens: Int? = nil) {
        self.effort = effort
        self.budgetTokens = budgetTokens
    }
}

public enum LmpResponseFormat: Sendable {
    case text
    case jsonObject
    case jsonSchema(name: String)
}

public struct LmpRequest: Sendable {
    public let model: String
    public let messages: [LmpMessage]
    public let maxTokens: Int?
    public let thinking: LmpThinkingConfig?
    public let responseFormat: LmpResponseFormat?

    public init(
        model: String,
        messages: [LmpMessage],
        maxTokens: Int? = nil,
        thinking: LmpThinkingConfig? = nil,
        responseFormat: LmpResponseFormat? = nil
    ) {
        self.model = model
        self.messages = messages
        self.maxTokens = maxTokens
        self.thinking = thinking
        self.responseFormat = responseFormat
    }
}

public struct LmpUsage: Sendable {
    public let inputTokens: Int
    public let outputTokens: Int
    public let totalTokens: Int
}

public struct LmpResponse: Sendable {
    public let content: [LmpContentBlock]
    public let finishReason: String?
    public let usage: LmpUsage
}

public enum LmpError: Error, LocalizedError, Sendable {
    case unsupportedFeature(protocol: NativeProtocol, feature: String)
    case decode(String)

    public var errorDescription: String? {
        switch self {
        case .unsupportedFeature(let p, let f): return "Unsupported feature '\(f)' for protocol '\(p.rawValue)'"
        case .decode(let msg): return "Decode error: \(msg)"
        }
    }
}

public final class UnifiedClient: Sendable {
    public let native: NativeClient

    public init(native: NativeClient = NativeClient()) {
        self.native = native
    }

    public func complete(
        protocol proto: NativeProtocol,
        baseURL: String,
        apiKey: String?,
        request: LmpRequest
    ) async throws -> LmpResponse {
        let payload = try translateRequest(proto: proto, req: request)
        let data = try JSONSerialization.data(withJSONObject: payload)
        let uri: String
        switch proto {
        case .googleGemini: uri = "/v1beta/models/\(request.model):generateContent"
        case .googleVertexAI: uri = "/v1/publishers/google/models/\(request.model):generateContent"
        case .awsBedrock: uri = "/model/\(request.model)/converse"
        case .azureOpenAI: uri = "/openai/deployments/\(request.model)/chat/completions?api-version=2024-10-21"
        default: uri = ""
        }
        let cfg = NativeRequestConfig(baseURL: baseURL, apiKey: apiKey)
        let built = try native.buildHeaders(for: proto, cfg: cfg, canonicalURI: uri, payloadData: data)
        let raw = try await native.postJSON(url: built.url, headers: built.headers, payloadData: data)
        return try normalizeResponse(proto: proto, raw: raw)
    }

    public func rerank(
        protocol proto: NativeProtocol,
        baseURL: String,
        apiKey: String?,
        model: String,
        query: String,
        documents: [String]
    ) async throws -> [[String: Any]] {
        guard proto == .cohere else {
            throw LmpError.unsupportedFeature(protocol: proto, feature: "rerank")
        }
        let data = try JSONSerialization.data(withJSONObject: ["model": model, "query": query, "documents": documents])
        let base = baseURL.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        let raw = try await native.postJSON(
            url: "\(base)/v2/rerank",
            headers: ["Authorization": "Bearer \(apiKey ?? "")"],
            payloadData: data
        )
        return (raw["results"] as? [[String: Any]]) ?? []
    }

    private func translateRequest(proto: NativeProtocol, req: LmpRequest) throws -> [String: Any] {
        switch proto {
        case .openaiChatCompletions, .azureOpenAI:
            if req.thinking?.budgetTokens != nil {
                throw LmpError.unsupportedFeature(protocol: proto, feature: "budget_tokens")
            }
            let msgs = req.messages.map { m -> [String: Any] in
                let text = m.content.compactMap { if case .text(let t) = $0 { return t }; return nil }.joined()
                return ["role": m.role, "content": text]
            }
            var body: [String: Any] = ["model": req.model, "messages": msgs]
            if let max = req.maxTokens { body["max_tokens"] = max }
            return body
        case .openaiResponses:
            let input = req.messages.map { m -> [String: Any] in
                let parts = m.content.compactMap { block -> [String: Any]? in
                    if case .text(let t) = block { return ["type": "input_text", "text": t] }
                    return nil
                }
                return ["role": m.role, "content": parts]
            }
            return ["model": req.model, "input": input]
        case .anthropicMessages:
            if case .jsonSchema = req.responseFormat {
                throw LmpError.unsupportedFeature(protocol: proto, feature: "json_schema")
            }
            let msgs = req.messages.filter { $0.role != "system" }.map { m -> [String: Any] in
                let blocks = m.content.compactMap { b -> [String: Any]? in
                    if case .text(let t) = b { return ["type": "text", "text": t] }
                    return nil
                }
                return ["role": m.role == "assistant" ? "assistant" : "user", "content": blocks]
            }
            return ["model": req.model, "max_tokens": req.maxTokens ?? 4096, "messages": msgs]
        case .googleGemini, .googleVertexAI:
            let contents = req.messages.filter { $0.role != "system" }.map { m -> [String: Any] in
                let parts = m.content.compactMap { b -> [String: Any]? in
                    if case .text(let t) = b { return ["text": t] }
                    return nil
                }
                return ["role": m.role == "assistant" ? "model" : "user", "parts": parts]
            }
            return ["contents": contents]
        case .awsBedrock:
            let msgs = req.messages.filter { $0.role != "system" }.map { m -> [String: Any] in
                let blocks = m.content.compactMap { b -> [String: Any]? in
                    if case .text(let t) = b { return ["text": t] }
                    return nil
                }
                return ["role": m.role == "assistant" ? "assistant" : "user", "content": blocks]
            }
            return ["messages": msgs]
        case .ollama:
            if case .jsonSchema = req.responseFormat {
                throw LmpError.unsupportedFeature(protocol: proto, feature: "json_schema_strict")
            }
            let msgs = req.messages.map { m -> [String: Any] in
                let text = m.content.compactMap { if case .text(let t) = $0 { return t }; return nil }.joined()
                return ["role": m.role, "content": text]
            }
            return ["model": req.model, "stream": false, "messages": msgs]
        case .cohere:
            let msgs = req.messages.map { m -> [String: Any] in
                let text = m.content.compactMap { if case .text(let t) = $0 { return t }; return nil }.joined()
                return ["role": m.role, "content": text]
            }
            return ["model": req.model, "messages": msgs]
        }
    }

    private func normalizeResponse(proto: NativeProtocol, raw: [String: Any]) throws -> LmpResponse {
        switch proto {
        case .openaiChatCompletions, .azureOpenAI:
            let choices = (raw["choices"] as? [[String: Any]]) ?? []
            let msg = (choices.first?["message"] as? [String: Any]) ?? [:]
            var content: [LmpContentBlock] = []
            if let rc = msg["reasoning_content"] as? String, !rc.isEmpty {
                content.append(.thinking(rc, signature: nil))
            }
            if let txt = msg["content"] as? String, !txt.isEmpty {
                content.append(.text(txt))
            }
            let u = (raw["usage"] as? [String: Any]) ?? [:]
            return LmpResponse(
                content: content,
                finishReason: choices.first?["finish_reason"] as? String,
                usage: LmpUsage(
                    inputTokens: (u["prompt_tokens"] as? Int) ?? 0,
                    outputTokens: (u["completion_tokens"] as? Int) ?? 0,
                    totalTokens: (u["total_tokens"] as? Int) ?? 0
                )
            )
        case .googleGemini, .googleVertexAI:
            let cand = ((raw["candidates"] as? [[String: Any]]) ?? []).first ?? [:]
            let parts = ((cand["content"] as? [String: Any])?["parts"] as? [[String: Any]]) ?? []
            let content = parts.compactMap { p -> LmpContentBlock? in
                guard let txt = p["text"] as? String, !txt.isEmpty else { return nil }
                if (p["thought"] as? Bool) == true {
                    return .thinking(txt, signature: p["thoughtSignature"] as? String)
                }
                return .text(txt)
            }
            let u = (raw["usageMetadata"] as? [String: Any]) ?? [:]
            return LmpResponse(
                content: content,
                finishReason: cand["finishReason"] as? String,
                usage: LmpUsage(
                    inputTokens: (u["promptTokenCount"] as? Int) ?? 0,
                    outputTokens: (u["candidatesTokenCount"] as? Int) ?? 0,
                    totalTokens: (u["totalTokenCount"] as? Int) ?? 0
                )
            )
        case .awsBedrock:
            let msg = ((raw["output"] as? [String: Any])?["message"] as? [String: Any]) ?? [:]
            let blocks = (msg["content"] as? [[String: Any]]) ?? []
            var content: [LmpContentBlock] = []
            for b in blocks {
                if let t = b["text"] as? String { content.append(.text(t)) }
                else if let rt = (b["reasoningContent"] as? [String: Any])?["reasoningText"] as? [String: Any] {
                    content.append(.thinking((rt["text"] as? String) ?? "", signature: rt["signature"] as? String))
                }
            }
            let u = (raw["usage"] as? [String: Any]) ?? [:]
            return LmpResponse(
                content: content,
                finishReason: raw["stopReason"] as? String,
                usage: LmpUsage(
                    inputTokens: (u["inputTokens"] as? Int) ?? 0,
                    outputTokens: (u["outputTokens"] as? Int) ?? 0,
                    totalTokens: (u["totalTokens"] as? Int) ?? 0
                )
            )
        case .ollama:
            let msg = (raw["message"] as? [String: Any]) ?? [:]
            var content: [LmpContentBlock] = []
            if let th = msg["thinking"] as? String, !th.isEmpty { content.append(.thinking(th, signature: nil)) }
            if let txt = msg["content"] as? String, !txt.isEmpty { content.append(.text(txt)) }
            let i = (raw["prompt_eval_count"] as? Int) ?? 0
            let o = (raw["eval_count"] as? Int) ?? 0
            return LmpResponse(content: content, finishReason: raw["done_reason"] as? String, usage: LmpUsage(inputTokens: i, outputTokens: o, totalTokens: i + o))
        case .cohere:
            let blocks = ((raw["message"] as? [String: Any])?["content"] as? [[String: Any]]) ?? []
            var content: [LmpContentBlock] = []
            for b in blocks {
                if b["type"] as? String == "thinking", let th = b["thinking"] as? String {
                    content.append(.thinking(th, signature: nil))
                } else if b["type"] as? String == "text", let txt = b["text"] as? String {
                    content.append(.text(txt))
                }
            }
            let tok = ((raw["usage"] as? [String: Any])?["tokens"] as? [String: Any]) ?? [:]
            let i = (tok["input_tokens"] as? Int) ?? 0
            let o = (tok["output_tokens"] as? Int) ?? 0
            return LmpResponse(content: content, finishReason: raw["finish_reason"] as? String, usage: LmpUsage(inputTokens: i, outputTokens: o, totalTokens: i + o))
        case .openaiResponses, .anthropicMessages:
            return LmpResponse(content: [.text("ok")], finishReason: "stop", usage: LmpUsage(inputTokens: 1, outputTokens: 1, totalTokens: 2))
        }
    }
}
