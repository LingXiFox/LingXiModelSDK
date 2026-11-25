import Foundation
import LingXiNativeProvider
import LingXiProviderRegistry

/// Unified (LMP) client: every provider is translated, sent and normalized by
/// the Rust core, so no capability is silently downgraded and no raw provider
/// JSON leaks out.
public final class UnifiedClient: Sendable {
    public let native: NativeClient

    public init(native: NativeClient = NativeClient()) {
        self.native = native
    }

    /// True when the Rust core shared library could be loaded.
    public static var isAvailable: Bool { RustCore.isAvailable }

    /// One blocking-normalization request: translate, send, normalize.
    public func complete(
        protocol proto: NativeProtocol,
        baseURL: String,
        apiKey: String?,
        request: LmpRequest
    ) async throws -> LmpResponse {
        let target = try prepareUnified(proto: proto, baseURL: baseURL, apiKey: apiKey, request: request)
        let raw = try await native.postJSON(
            url: target.url,
            headers: target.headers,
            payloadData: target.bodyData
        )
        return try normalize(proto: proto, raw: raw)
    }

    public func completeText(
        protocol proto: NativeProtocol,
        baseURL: String,
        apiKey: String?,
        model: String,
        prompt: String
    ) async throws -> LmpResponse {
        try await complete(
            protocol: proto,
            baseURL: baseURL,
            apiKey: apiKey,
            request: LmpRequest(model: model, messages: [LmpMessage(role: "user", text: prompt)])
        )
    }

    /// Incremental Unified streaming: each yielded event is a typed LMP event.
    ///
    /// Transport, framing (SSE / NDJSON / Amazon Event Stream) and protocol
    /// state live in the core, so tool-call argument fragments, thinking
    /// signatures and usage are assembled exactly once.
    public func stream(
        protocol proto: NativeProtocol,
        baseURL: String,
        apiKey: String?,
        request: LmpRequest,
        headers customHeaders: [String: String] = [:],
        idleTimeout: TimeInterval = 120
    ) -> AsyncThrowingStream<LmpStreamEvent, Error> {
        var wire: [String: Any] = [
            "protocol": proto.rawValue,
            "baseUrl": baseURL,
            "payload": request.json(),
            "unified": true,
            "idleTimeoutMs": Int(idleTimeout * 1000),
        ]
        if let apiKey { wire["apiKey"] = apiKey }
        if !customHeaders.isEmpty { wire["headers"] = customHeaders }
        let protoName = proto.rawValue
        return AsyncThrowingStream { continuation in
            let opened: CoreStreamSession
            do {
                opened = try CoreStreamSession.open(wire, unified: true)
            } catch {
                continuation.finish(throwing: Self.mapCoreError(error, protocol: protoName))
                return
            }
            let worker = Task.detached(priority: .userInitiated) {
                defer { opened.close() }
                while !Task.isCancelled {
                    do {
                        guard let frame = try opened.next() else { break }
                        guard let event = LmpStreamEvent.decode(frame.json) else { continue }
                        continuation.yield(event)
                        if case .error = event { break }
                    } catch {
                        continuation.finish(throwing: Self.mapCoreError(error, protocol: protoName))
                        return
                    }
                }
                continuation.finish()
            }
            continuation.onTermination = { _ in
                opened.cancel()
                worker.cancel()
            }
        }
    }

    /// Collects a stream into a single LMP response.
    public func streamAll(
        protocol proto: NativeProtocol,
        baseURL: String,
        apiKey: String?,
        request: LmpRequest
    ) async throws -> LmpResponse {
        var text = ""
        var thinking = ""
        var toolCalls: [Int: (id: String, name: String, arguments: String)] = [:]
        var finishReason: String?
        var usage = LmpUsage(inputTokens: 0, outputTokens: 0, totalTokens: 0)
        for try await event in stream(protocol: proto, baseURL: baseURL, apiKey: apiKey, request: request) {
            switch event {
            case .textDelta(let delta, _): text += delta
            case .thinkingDelta(let delta, _): thinking += delta
            case .toolCallDelta(let index, let id, let name, let argumentsDelta, _):
                var current = toolCalls[index] ?? (id: "", name: "", arguments: "")
                if let id { current.id = id }
                if let name { current.name = name }
                current.arguments += argumentsDelta
                toolCalls[index] = current
            case .usage(let value): usage = value
            case .messageEnd(let reason): finishReason = reason
            case .error(let code, let message):
                throw LmpError.fromCoreCode(code, message, protocol: proto.rawValue)
            case .messageStart: break
            }
        }
        var content: [LmpContentBlock] = []
        if !thinking.isEmpty { content.append(.thinking(thinking, signature: nil)) }
        if !text.isEmpty { content.append(.text(text)) }
        for (_, call) in toolCalls.sorted(by: { $0.key < $1.key }) {
            content.append(.toolCall(id: call.id, name: call.name, argumentsJSON: call.arguments))
        }
        return LmpResponse(
            id: nil,
            model: request.model,
            content: content,
            finishReason: finishReason,
            usage: usage
        )
    }

    public func embed(
        protocol proto: NativeProtocol,
        baseURL: String,
        apiKey: String?,
        model: String,
        input: [String]
    ) throws -> LmpEmbeddingResponse {
        var request: [String: Any] = [
            "protocol": proto.rawValue,
            "baseUrl": baseURL,
            "model": model,
            "input": input,
        ]
        if let apiKey { request["apiKey"] = apiKey }
        do {
            let response = try RustCore.shared().embed(request)
            return LmpEmbeddingResponse.decode(response)
        } catch {
            throw Self.mapCoreError(error, protocol: proto.rawValue)
        }
    }

    public func rerank(
        protocol proto: NativeProtocol,
        baseURL: String,
        apiKey: String?,
        model: String,
        query: String,
        documents: [String],
        topN: Int? = nil
    ) throws -> LmpRerankResponse {
        var request: [String: Any] = [
            "protocol": proto.rawValue,
            "baseUrl": baseURL,
            "model": model,
            "query": query,
            "documents": documents,
        ]
        if let apiKey { request["apiKey"] = apiKey }
        if let topN { request["topN"] = topN }
        do {
            let response = try RustCore.shared().rerank(request)
            return LmpRerankResponse.decode(response)
        } catch {
            throw Self.mapCoreError(error, protocol: proto.rawValue)
        }
    }

    // MARK: - Core plumbing

    private func prepareUnified(
        proto: NativeProtocol,
        baseURL: String,
        apiKey: String?,
        request: LmpRequest
    ) throws -> NativeWireTarget {
        var wire: [String: Any] = [
            "protocol": proto.rawValue,
            "baseUrl": baseURL,
            "model": request.model,
            "request": request.json(),
        ]
        if let apiKey { wire["apiKey"] = apiKey }
        do {
            return try RustCore.shared().prepareUnified(wire)
        } catch {
            throw Self.mapCoreError(error, protocol: proto.rawValue)
        }
    }

    private func normalize(proto: NativeProtocol, raw: [String: Any]) throws -> LmpResponse {
        do {
            let normalized = try RustCore.shared().normalizeResponse(protocol: proto.rawValue, raw: raw)
            return try LmpResponse.decode(normalized)
        } catch {
            throw Self.mapCoreError(error, protocol: proto.rawValue)
        }
    }

    static func mapCoreError(_ error: Error, protocol proto: String) -> Error {
        if let core = error as? RustCoreError {
            switch core {
            case .unavailable(let reason): return LmpError.transport(reason)
            case .failure(_, let code, let message): return LmpError.fromCoreCode(code, message, protocol: proto)
            }
        }
        return error
    }
}
