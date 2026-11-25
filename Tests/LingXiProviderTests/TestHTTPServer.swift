import Foundation
#if canImport(Darwin)
import Darwin
#else
import Glibc
#endif

struct TestHTTPRequest: Sendable {
    let method: String
    let path: String
    let headers: [String: String]
    let body: Data

    var json: [String: Any] {
        (try? JSONSerialization.jsonObject(with: body)) as? [String: Any] ?? [:]
    }
}

struct TestHTTPResponse: Sendable {
    let status: Int
    let contentType: String
    var chunks: [Data]
    /// Delay applied after each chunk except the last, in milliseconds.
    var delayMilliseconds: Int = 0

    init(status: Int, contentType: String, chunks: [Data], delayMilliseconds: Int = 0) {
        self.status = status
        self.contentType = contentType
        self.chunks = chunks
        self.delayMilliseconds = delayMilliseconds
    }

    static func body(_ text: String, contentType: String = "application/json", status: Int = 200) -> TestHTTPResponse {
        TestHTTPResponse(status: status, contentType: contentType, chunks: [Data(text.utf8)])
    }

    static func file(_ path: String, contentType: String) throws -> TestHTTPResponse {
        let data = try Data(contentsOf: URL(fileURLWithPath: path))
        return TestHTTPResponse(status: 200, contentType: contentType, chunks: [data])
    }
}

/// Minimal loopback HTTP/1.1 server for provider mock tests.
///
/// Responses without a Content-Length use `Connection: close`, so the client
/// sees body bytes as they are written: that is how incremental streaming and
/// cancellation are exercised without external tooling.
final class TestHTTPServer: @unchecked Sendable {
    let port: Int
    private let listenFD: Int32
    private let route: @Sendable (TestHTTPRequest) throws -> TestHTTPResponse
    private let stopFlag = Flag()

    final class Flag: @unchecked Sendable {
        private let lock = NSLock()
        private var value = false
        func set() { lock.lock(); value = true; lock.unlock() }
        func get() -> Bool { lock.lock(); defer { lock.unlock() }; return value }
    }

    init(route: @escaping @Sendable (TestHTTPRequest) throws -> TestHTTPResponse) throws {
        #if canImport(Darwin)
        let fd = socket(AF_INET, SOCK_STREAM, 0)
        #else
        let fd = socket(AF_INET, Int32(SOCK_STREAM.rawValue), 0)
        #endif
        guard fd >= 0 else { throw TestServerError.socket("socket() failed") }
        var yes: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &yes, socklen_t(MemoryLayout<Int32>.size))
        var addr = sockaddr_in()
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = 0
        addr.sin_addr.s_addr = inet_addr("127.0.0.1")
        let bound = withUnsafePointer(to: &addr) { pointer -> Int32 in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { raw in
                #if canImport(Darwin)
                bind(fd, raw, socklen_t(MemoryLayout<sockaddr_in>.size))
                #else
                bind(fd, raw, socklen_t(MemoryLayout<sockaddr_in>.size))
                #endif
            }
        }
        guard bound == 0, listen(fd, 16) == 0 else {
            close(fd)
            throw TestServerError.socket("bind/listen failed")
        }
        var actual = sockaddr_in()
        var length = socklen_t(MemoryLayout<sockaddr_in>.size)
        _ = withUnsafeMutablePointer(to: &actual) { pointer -> Int32 in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { raw in
                getsockname(fd, raw, &length)
            }
        }
        self.listenFD = fd
        self.port = Int(UInt16(bigEndian: actual.sin_port))
        self.route = route
        let worker = Thread { [self] in serve() }
        worker.stackSize = 512 * 1024
        worker.start()
    }

    var baseURL: String { "http://127.0.0.1:\(port)" }

    func stop() {
        stopFlag.set()
        #if canImport(Darwin)
        Darwin.shutdown(listenFD, SHUT_RDWR)
        #else
        Glibc.shutdown(listenFD, Int32(SHUT_RDWR))
        #endif
        close(listenFD)
    }

    private func serve() {
        while !stopFlag.get() {
            var clientAddr = sockaddr()
            var length = socklen_t(MemoryLayout<sockaddr>.size)
            let client = accept(listenFD, &clientAddr, &length)
            if client < 0 { return }
            handle(client: client)
        }
    }

    private func handle(client: Int32) {
        defer { close(client) }
        #if canImport(Darwin)
        // A client that hangs up mid-stream must not raise SIGPIPE and kill the test process.
        var noSignal: Int32 = 1
        setsockopt(client, SOL_SOCKET, SO_NOSIGPIPE, &noSignal, socklen_t(MemoryLayout<Int32>.size))
        #endif
        guard let request = readRequest(client) else { return }
        do {
            let response = try route(request)
            writeResponse(response, client: client)
        } catch {
            writeResponse(TestHTTPResponse.body("{\"error\":\"server\"}", status: 500), client: client)
        }
    }

    private func readRequest(_ client: Int32) -> TestHTTPRequest? {
        var buffer = Data()
        var scratch = [UInt8](repeating: 0, count: 8192)
        while true {
            let read = recv(client, &scratch, scratch.count, 0)
            if read <= 0 { return nil }
            buffer.append(contentsOf: scratch[0..<read])
            if let range = buffer.range(of: Data("\r\n\r\n".utf8)) {
                let headerData = buffer.subdata(in: 0..<range.lowerBound)
                let bodyStart = range.upperBound
                guard let headerText = String(data: headerData, encoding: .utf8) else { return nil }
                let lines = headerText.components(separatedBy: "\r\n")
                guard let requestLine = lines.first else { return nil }
                let parts = requestLine.split(separator: " ")
                guard parts.count >= 2 else { return nil }
                var headers: [String: String] = [:]
                for line in lines.dropFirst() {
                    guard let colon = line.firstIndex(of: ":") else { continue }
                    let key = String(line[..<colon]).lowercased()
                    let value = String(line[line.index(after: colon)...]).trimmingCharacters(in: .whitespaces)
                    headers[key] = value
                }
                let expected = Int(headers["content-length"] ?? "0") ?? 0
                var body = buffer.subdata(in: bodyStart..<buffer.count)
                while body.count < expected {
                    let read = recv(client, &scratch, scratch.count, 0)
                    if read <= 0 { break }
                    body.append(contentsOf: scratch[0..<read])
                }
                let path = String(parts[1])
                return TestHTTPRequest(method: String(parts[0]), path: path, headers: headers, body: body)
            }
            if buffer.count > 32 * 1024 * 1024 { return nil }
        }
    }

    private func writeResponse(_ response: TestHTTPResponse, client: Int32) {
        var header = "HTTP/1.1 \(response.status) OK\r\n"
        header += "Content-Type: \(response.contentType)\r\n"
        header += "Connection: close\r\n\r\n"
        writeAll(client, Array(header.utf8))
        for (index, chunk) in response.chunks.enumerated() {
            writeAll(client, Array(chunk))
            if index + 1 < response.chunks.count, response.delayMilliseconds > 0 {
                Thread.sleep(forTimeInterval: Double(response.delayMilliseconds) / 1000)
            }
        }
    }

    private func writeAll(_ client: Int32, _ bytes: [UInt8]) {
        var offset = 0
        while offset < bytes.count {
            let written = bytes[offset...].withUnsafeBytes { raw in
                Darwin.send(client, raw.baseAddress, raw.count, 0)
            }
            if written <= 0 { return }
            offset += written
        }
    }

    deinit { stop() }
}

enum TestServerError: Error { case socket(String) }

func fixturePath(_ name: String) throws -> String {
    let root = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .deletingLastPathComponent()
    return root.appendingPathComponent("conformance/native/\(name)").path
}

func fixtureText(_ name: String) throws -> String {
    try String(contentsOfFile: fixturePath(name), encoding: .utf8)
}

/// Splits an SSE/NDJSON fixture into the frames the mock server streams.
func frames(_ text: String, ndjson: Bool = false) -> [Data] {
    if ndjson {
        return text.split(separator: "\n").map { Data(($0 + "\n").utf8) }
    }
    var out: [Data] = []
    var current: [String] = []
    for line in text.split(separator: "\n", omittingEmptySubsequences: false) {
        if line.isEmpty {
            if !current.isEmpty {
                out.append(Data((current.joined(separator: "\n") + "\n\n").utf8))
                current = []
            }
        } else {
            current.append(String(line))
        }
    }
    if !current.isEmpty { out.append(Data((current.joined(separator: "\n") + "\n\n").utf8)) }
    return out
}
