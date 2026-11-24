import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
import LingXiProviderRegistry

public struct NativeRequestConfig: Sendable {
    public let baseURL: String
    public let apiKey: String?
    public let headers: [String: String]
    public let allowInsecureHTTP: Bool

    public init(
        baseURL: String,
        apiKey: String? = nil,
        headers: [String: String] = [:],
        allowInsecureHTTP: Bool = false
    ) {
        self.baseURL = baseURL
        self.apiKey = apiKey
        self.headers = headers
        self.allowInsecureHTTP = allowInsecureHTTP
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

    public var errorDescription: String? {
        switch self {
        case .authMissing(let msg): return "Missing auth: \(msg)"
        case .httpStatus(let s, let b): return "Upstream HTTP \(s): \(b)"
        case .redirectBlocked(let s): return "Redirect HTTP \(s) blocked"
        case .invalidResponse(let msg): return "Invalid response: \(msg)"
        }
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

public final class NativeClient: Sendable {
    private let session: URLSession
    private let delegate = NoRedirectDelegate()

    public init(configuration: URLSessionConfiguration = .default) {
        self.session = URLSession(configuration: configuration, delegate: delegate, delegateQueue: nil)
    }

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

    public func postStreamLines(
        url rawURL: String,
        headers: [String: String],
        payloadData: Data,
        ndjson: Bool = false,
        allowInsecureHTTP: Bool = false
    ) async throws -> [SSEEvent] {
        try validateEndpointURL(rawURL, allowInsecureHTTP: allowInsecureHTTP)
        guard let url = URL(string: rawURL) else {
            throw RegistryError.invalidEndpoint(rawURL)
        }
        var req = URLRequest(url: url)
        req.httpMethod = "POST"
        req.httpBody = payloadData
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.setValue(ndjson ? "application/x-ndjson, application/json" : "text/event-stream", forHTTPHeaderField: "Accept")
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
        let text = String(data: data, encoding: .utf8) ?? ""
        return ndjson ? Self.parseNDJSON(text) : Self.parseSSE(text)
    }

    public static func parseSSE(_ text: String) -> [SSEEvent] {
        var events: [SSEEvent] = []
        var eventName: String?
        var dataLines: [String] = []
        var lastID: String?
        func flush() {
            guard !dataLines.isEmpty || eventName != nil else { return }
            events.append(SSEEvent(event: eventName, data: dataLines.joined(separator: "\n"), id: lastID))
            eventName = nil
            dataLines = []
        }
        for rawLine in text.split(separator: "\n", omittingEmptySubsequences: false) {
            let line = rawLine.hasSuffix("\r") ? String(rawLine.dropLast()) : String(rawLine)
            if line.isEmpty { flush(); continue }
            if line.hasPrefix(":") { continue }
            if let idx = line.firstIndex(of: ":") {
                let field = String(line[..<idx])
                var val = String(line[line.index(after: idx)...])
                if val.hasPrefix(" ") { val.removeFirst() }
                if field == "event" { eventName = val }
                else if field == "data" { dataLines.append(val) }
                else if field == "id" { lastID = val }
            }
        }
        flush()
        return events
    }

    public static func parseNDJSON(_ text: String) -> [SSEEvent] {
        text.split(separator: "\n")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
            .map { SSEEvent(event: nil, data: $0, id: nil) }
    }

    public func buildHeaders(
        for proto: NativeProtocol,
        cfg: NativeRequestConfig,
        canonicalURI: String,
        payloadData: Data
    ) throws -> (url: String, headers: [String: String]) {
        var headers = cfg.headers
        let base = cfg.baseURL.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        switch proto {
        case .openaiChatCompletions:
            headers["Authorization"] = try requireKey(cfg.apiKey).map { "Bearer \($0)" }
            return ("\(base)/chat/completions", headers)
        case .openaiResponses:
            headers["Authorization"] = try requireKey(cfg.apiKey).map { "Bearer \($0)" }
            return ("\(base)/responses", headers)
        case .anthropicMessages:
            headers["x-api-key"] = try requireKey(cfg.apiKey)
            if headers["anthropic-version"] == nil { headers["anthropic-version"] = "2023-06-01" }
            let url = base.hasSuffix("/v1") ? "\(base)/messages" : "\(base)/v1/messages"
            return (url, headers)
        case .googleGemini:
            headers["x-goog-api-key"] = try requireKey(cfg.apiKey)
            return ("\(base)\(canonicalURI)", headers)
        case .googleVertexAI:
            headers["Authorization"] = "Bearer \(try requireKey(cfg.apiKey))"
            return ("\(base)\(canonicalURI)", headers)
        case .azureOpenAI:
            headers["api-key"] = try requireKey(cfg.apiKey)
            return ("\(base)\(canonicalURI)", headers)
        case .ollama:
            return ("\(base)/api/chat", headers)
        case .cohere:
            headers["Authorization"] = "Bearer \(try requireKey(cfg.apiKey))"
            return ("\(base)/v2/chat", headers)
        case .awsBedrock:
            let key = try requireKey(cfg.apiKey)
            let parts = key.split(separator: ":").map(String.init)
            if parts.count >= 2, let u = URL(string: "\(base)\(canonicalURI)"), let host = u.host {
                let hostPort = u.port.map { "\(host):\($0)" } ?? host
                let region = headers["x-aws-region"] ?? "us-east-1"
                let signed = SigV4.sign(
                    method: "POST",
                    canonicalURI: canonicalURI,
                    host: hostPort,
                    region: region,
                    service: "bedrock",
                    accessKey: parts[0],
                    secretKey: parts[1],
                    sessionToken: parts.count >= 3 ? parts[2] : nil,
                    payload: payloadData
                )
                for (k, v) in signed { headers[k] = v }
            } else {
                headers["Authorization"] = "Bearer \(key)"
            }
            return ("\(base)\(canonicalURI)", headers)
        }
    }

    private func requireKey(_ key: String?) throws -> String {
        guard let k = key, !k.isEmpty else {
            throw NativeClientError.authMissing("API key is required")
        }
        return k
    }
}

private extension String {
    func map(_ transform: (String) -> String) -> String { transform(self) }
}

/// Pure Swift SHA-256 + HMAC-SHA256 + AWS SigV4 (portable across macOS and Linux without external deps).
public enum SigV4 {
    private static let k: [UInt32] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ]

    public static func sha256(_ data: [UInt8]) -> [UInt8] {
        var h: [UInt32] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19]
        var msg = data
        let bitLen = UInt64(data.count) &* 8
        msg.append(0x80)
        while msg.count % 64 != 56 { msg.append(0) }
        for i in stride(from: 56, through: 0, by: -8) {
            msg.append(UInt8((bitLen >> i) & 0xff))
        }
        func rotr(_ x: UInt32, _ n: UInt32) -> UInt32 { (x >> n) | (x << (32 - n)) }
        for off in stride(from: 0, to: msg.count, by: 64) {
            var w = [UInt32](repeating: 0, count: 64)
            for i in 0..<16 {
                let j = off + i * 4
                w[i] = (UInt32(msg[j]) << 24) | (UInt32(msg[j+1]) << 16) | (UInt32(msg[j+2]) << 8) | UInt32(msg[j+3])
            }
            for i in 16..<64 {
                let s0 = rotr(w[i-15], 7) ^ rotr(w[i-15], 18) ^ (w[i-15] >> 3)
                let s1 = rotr(w[i-2], 17) ^ rotr(w[i-2], 19) ^ (w[i-2] >> 10)
                w[i] = w[i-16] &+ s0 &+ w[i-7] &+ s1
            }
            var (a, b, c, d, e, f, g, hh) = (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7])
            for i in 0..<64 {
                let s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25)
                let ch = (e & f) ^ ((~e) & g)
                let t1 = hh &+ s1 &+ ch &+ k[i] &+ w[i]
                let s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22)
                let maj = (a & b) ^ (a & c) ^ (b & c)
                let t2 = s0 &+ maj
                hh = g; g = f; f = e; e = d &+ t1; d = c; c = b; b = a; a = t1 &+ t2
            }
            h[0] = h[0] &+ a; h[1] = h[1] &+ b; h[2] = h[2] &+ c; h[3] = h[3] &+ d
            h[4] = h[4] &+ e; h[5] = h[5] &+ f; h[6] = h[6] &+ g; h[7] = h[7] &+ hh
        }
        return h.flatMap { [UInt8(($0 >> 24) & 0xff), UInt8(($0 >> 16) & 0xff), UInt8(($0 >> 8) & 0xff), UInt8($0 & 0xff)] }
    }

    public static func hmacSha256(key: [UInt8], data: [UInt8]) -> [UInt8] {
        var kBlock = [UInt8](repeating: 0, count: 64)
        let norm = key.count > 64 ? sha256(key) : key
        for i in 0..<norm.count { kBlock[i] = norm[i] }
        let oPad = kBlock.map { $0 ^ 0x5c }
        let iPad = kBlock.map { $0 ^ 0x36 }
        return sha256(oPad + sha256(iPad + data))
    }

    public static func hex(_ bytes: [UInt8]) -> String {
        bytes.map { String(format: "%02x", $0) }.joined()
    }

    public static func sign(
        method: String,
        canonicalURI: String,
        host: String,
        region: String,
        service: String,
        accessKey: String,
        secretKey: String,
        sessionToken: String?,
        payload: Data
    ) -> [String: String] {
        let fmt = DateFormatter()
        fmt.locale = Locale(identifier: "en_US_POSIX")
        fmt.timeZone = TimeZone(secondsFromGMT: 0)
        fmt.dateFormat = "yyyyMMdd'T'HHmmss'Z'"
        let amzDate = fmt.string(from: Date())
        let dateStamp = String(amzDate.prefix(8))
        let payloadHash = hex(sha256([UInt8](payload)))
        var hdrs = ["host": host, "x-amz-date": amzDate]
        if let tok = sessionToken, !tok.isEmpty { hdrs["x-amz-security-token"] = tok }
        let keys = hdrs.keys.sorted()
        let canonicalHeaders = keys.map { "\($0):\(hdrs[$0]!)\n" }.joined()
        let signedHeaders = keys.joined(separator: ";")
        let canonicalReq = "\(method)\n\(canonicalURI)\n\n\(canonicalHeaders)\n\(signedHeaders)\n\(payloadHash)"
        let scope = "\(dateStamp)/\(region)/\(service)/aws4_request"
        let stringToSign = "AWS4-HMAC-SHA256\n\(amzDate)\n\(scope)\n\(hex(sha256([UInt8](canonicalReq.utf8))))"
        let kDate = hmacSha256(key: [UInt8]("AWS4\(secretKey)".utf8), data: [UInt8](dateStamp.utf8))
        let kRegion = hmacSha256(key: kDate, data: [UInt8](region.utf8))
        let kService = hmacSha256(key: kRegion, data: [UInt8](service.utf8))
        let kSigning = hmacSha256(key: kService, data: [UInt8]("aws4_request".utf8))
        let sig = hex(hmacSha256(key: kSigning, data: [UInt8](stringToSign.utf8)))
        var out = [
            "Authorization": "AWS4-HMAC-SHA256 Credential=\(accessKey)/\(scope), SignedHeaders=\(signedHeaders), Signature=\(sig)",
            "X-Amz-Date": amzDate,
        ]
        if let tok = sessionToken, !tok.isEmpty { out["X-Amz-Security-Token"] = tok }
        return out
    }
}
