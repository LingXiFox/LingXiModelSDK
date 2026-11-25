import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
import LingXiProviderRegistry

/// Transport configuration for a single provider request.
public struct NativeRequestConfig: Sendable {
    public let baseURL: String
    public let apiKey: String?
    public let headers: [String: String]
    public let allowInsecureHTTP: Bool
    /// Maximum silence tolerated between two streamed events.
    public let streamIdleTimeout: TimeInterval
    public let timeout: TimeInterval

    public init(
        baseURL: String,
        apiKey: String? = nil,
        headers: [String: String] = [:],
        allowInsecureHTTP: Bool = false,
        timeout: TimeInterval = 60,
        streamIdleTimeout: TimeInterval = 120
    ) {
        self.baseURL = baseURL
        self.apiKey = apiKey
        self.headers = headers
        self.allowInsecureHTTP = allowInsecureHTTP
        self.timeout = timeout
        self.streamIdleTimeout = streamIdleTimeout
    }
}

public struct SSEEvent: Sendable, Equatable {
    public let event: String?
    public let data: String
    public let id: String?

    public init(event: String?, data: String, id: String?) {
        self.event = event
        self.data = data
        self.id = id
    }
}

public enum NativeClientError: Error, LocalizedError, Sendable {
    case authMissing(String)
    case httpStatus(status: Int, body: String)
    case redirectBlocked(Int)
    case invalidResponse(String)
    case core(String, String)
    case cancelled(String)

    public var errorDescription: String? {
        switch self {
        case .authMissing(let msg): return "Missing auth: \(msg)"
        case .httpStatus(let s, let b): return "Upstream HTTP \(s): \(b)"
        case .redirectBlocked(let s): return "Redirect HTTP \(s) blocked"
        case .invalidResponse(let msg): return "Invalid response: \(msg)"
        case .core(let code, let msg): return "Core failure [\(code)]: \(msg)"
        case .cancelled(let msg): return "Cancelled: \(msg)"
        }
    }

    public var code: String? {
        if case .core(let code, _) = self { return code }
        return nil
    }
}

private final class NoRedirectDelegate: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest,
        completionHandler: @escaping (URLRequest?) -> Void
    ) {
        completionHandler(nil)
    }
}

/// Native provider transport.
///
/// Endpoint joining, protocol auth headers, AWS SigV4 and stream framing are
/// produced by the Rust core (see `RustCore.prepareNative` and the core stream
/// handles), so this type only performs HTTP I/O and surfaces events.
public final class NativeClient: Sendable {
    private let session: URLSession
    private let delegate = NoRedirectDelegate()

    public init(configuration: URLSessionConfiguration = .default) {
        self.session = URLSession(configuration: configuration, delegate: delegate, delegateQueue: nil)
    }

    /// POSTs a provider payload and returns the decoded JSON object.
    public func postJSON(
        url rawURL: String,
        headers: [String: String],
        payloadData: Data,
        allowInsecureHTTP: Bool = false
    ) async throws -> [String: Any] {
        try validateEndpointURL(rawURL, allowInsecureHTTP: allowInsecureHTTP)
        guard let url = URL(string: rawURL) else {
            throw RegistryError.invalidEndpoint(rawURL)
        }
        var req = URLRequest(url: url)
        req.httpMethod = "POST"
        req.httpBody = payloadData
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.setValue("application/json", forHTTPHeaderField: "Accept")
        for (k, v) in headers {
            req.setValue(v, forHTTPHeaderField: k)
        }
        let (data, resp) = try await session.data(for: req)
        guard let http = resp as? HTTPURLResponse else {
            throw NativeClientError.invalidResponse("non-HTTP response")
        }
        if (300...399).contains(http.statusCode) {
            throw NativeClientError.redirectBlocked(http.statusCode)
        }
        guard (200...299).contains(http.statusCode) else {
            throw NativeClientError.httpStatus(
                status: http.statusCode,
                body: String(data: data, encoding: .utf8) ?? ""
            )
        }
        guard let obj = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw NativeClientError.invalidResponse("expected JSON object")
        }
        return obj
    }

    /// Sends a provider payload through the core and streams raw framing back.
    ///
    /// Returns the provider's own events (`event`/`data`/`id`); the `[DONE]`
    /// sentinel some providers append is filtered out. Decoding, SigV4 and
    /// Amazon Event Stream framing all happen inside the core.
    public func streamEvents(
        protocol proto: NativeProtocol,
        cfg: NativeRequestConfig,
        model: String?,
        payload: [String: Any],
        headers customHeaders: [String: String] = [:]
    ) -> AsyncThrowingStream<SSEEvent, Error> {
        var request: [String: Any] = [
            "protocol": proto.rawValue,
            "baseUrl": cfg.baseURL,
            "payload": payload,
            "stream": true,
            "allowInsecureHttp": cfg.allowInsecureHTTP,
            "idleTimeoutMs": Int(cfg.streamIdleTimeout * 1000),
            "timeoutMs": Int(cfg.timeout * 1000),
        ]
        if let model { request["model"] = model }
        if let key = cfg.apiKey { request["apiKey"] = key }
        let merged = cfg.headers.merging(customHeaders) { _, new in new }
        if !merged.isEmpty { request["headers"] = merged }
        return Self.rawFrames(request)
    }

    /// Streams the raw frames of an already-built core stream request.
    static func rawFrames(_ request: [String: Any], unified: Bool = false) -> AsyncThrowingStream<SSEEvent, Error> {
        AsyncThrowingStream { continuation in
            let opened: CoreStreamSession
            do {
                opened = try CoreStreamSession.open(request, unified: unified)
            } catch {
                continuation.finish(throwing: error)
                return
            }
            let worker = Task.detached(priority: .userInitiated) {
                defer { opened.close() }
                while !Task.isCancelled {
                    do {
                        guard let frame = try opened.next() else { break }
                        let data = frame.json["data"] as? String ?? ""
                        if data.trimmingCharacters(in: .whitespacesAndNewlines) == "[DONE]" { continue }
                        continuation.yield(SSEEvent(
                            event: frame.json["event"] as? String,
                            data: data,
                            id: frame.json["id"] as? String
                        ))
                    } catch {
                        continuation.finish(throwing: Self.mapCoreError(error))
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

    static func mapCoreError(_ error: Error) -> NativeClientError {
        if let core = error as? RustCoreError, case .failure(let op, let code, let message) = core {
            if code == "cancelled" { return .cancelled(message) }
            return .core(code, "\(op): \(message)")
        }
        return .core("error", String(describing: error))
    }

    public static func parseSSE(_ text: String) -> [SSEEvent] {
        var parser = SSELineParser()
        var events: [SSEEvent] = []
        for line in text.split(separator: "\n", omittingEmptySubsequences: false) {
            events.append(contentsOf: parser.push(String(line)))
        }
        events.append(contentsOf: parser.finish())
        return events
    }

    public static func parseNDJSON(_ text: String) -> [SSEEvent] {
        text.split(separator: "\n")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
            .map { SSEEvent(event: nil, data: $0, id: nil) }
    }
}

/// Incremental SSE line parser: feed lines as they arrive, get finished events.
struct SSELineParser {
    private var eventName: String?
    private var dataLines: [String] = []
    private var lastID: String?

    mutating func push(_ rawLine: String) -> [SSEEvent] {
        let line = rawLine.hasSuffix("\r") ? String(rawLine.dropLast()) : rawLine
        if line.isEmpty { return flush() }
        if line.hasPrefix(":") { return [] }
        guard let idx = line.firstIndex(of: ":") else { return [] }
        let field = String(line[..<idx])
        var value = String(line[line.index(after: idx)...])
        if value.hasPrefix(" ") { value.removeFirst() }
        switch field {
        case "event": eventName = value
        case "data": dataLines.append(value)
        case "id": lastID = value
        default: break
        }
        return []
    }

    mutating func finish() -> [SSEEvent] { flush() }

    private mutating func flush() -> [SSEEvent] {
        guard !dataLines.isEmpty || eventName != nil else { return [] }
        let event = SSEEvent(event: eventName, data: dataLines.joined(separator: "\n"), id: lastID)
        eventName = nil
        dataLines = []
        return [event]
    }
}
