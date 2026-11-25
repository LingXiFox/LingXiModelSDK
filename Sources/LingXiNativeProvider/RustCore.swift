import Foundation
#if canImport(Darwin)
import Darwin
#else
import Glibc
#endif

/// A resolved upstream request: final URL, auth headers and native JSON body
/// produced by the Rust core, so no host language re-implements endpoint
/// joining, protocol auth headers or AWS SigV4.
public struct NativeWireTarget: Sendable {
    public let url: String
    public let headers: [String: String]
    public let bodyData: Data
    public let framing: String
    public let protocolName: String?

    public var body: Any? {
        try? JSONSerialization.jsonObject(with: bodyData, options: [])
    }

    public init(url: String, headers: [String: String], bodyData: Data, framing: String, protocolName: String?) {
        self.url = url
        self.headers = headers
        self.bodyData = bodyData
        self.framing = framing
        self.protocolName = protocolName
    }
}

public enum RustCoreError: Error, LocalizedError, Sendable {
    case unavailable(String)
    case failure(operation: String, code: String, message: String)

    public var errorDescription: String? {
        switch self {
        case .unavailable(let reason):
            return "lingxi_core shared library unavailable: \(reason)"
        case .failure(let operation, let code, let message):
            return "\(operation) failed [\(code)]: \(message)"
        }
    }

    public var code: String? {
        if case .failure(_, let code, _) = self { return code }
        return nil
    }
}

/// One frame read from a core stream handle.
public final class CoreStreamFrame: @unchecked Sendable {
    public let json: [String: Any]
    public let done: Bool

    init(json: [String: Any], done: Bool) {
        self.json = json
        self.done = done
    }
}

/// A core-owned incremental stream (native framing or LMP events).
///
/// `next` blocks, so poll it from a dedicated thread; the handle registry in
/// the core keeps close/use-after-close/close-while-reading safe.
public final class CoreStreamSession: @unchecked Sendable {
    private let core: RustCore
    private let lock = NSLock()
    private var handle: UnsafeMutableRawPointer?

    private init(core: RustCore, handle: UnsafeMutableRawPointer) {
        self.core = core
        self.handle = handle
    }

    public static func open(_ request: [String: Any], unified: Bool) throws -> CoreStreamSession {
        let core = try RustCore.shared()
        var body = request
        body["unified"] = unified
        let handle = try core.openStream(body)
        return CoreStreamSession(core: core, handle: handle)
    }

    /// Returns the next event, or nil once the upstream response has ended.
    public func next(timeoutMilliseconds: UInt64 = 600_000) throws -> CoreStreamFrame? {
        lock.lock()
        let current = handle
        lock.unlock()
        guard let current else { return nil }
        return try core.streamNext(current, timeoutMilliseconds: timeoutMilliseconds)
    }

    public func cancel() {
        lock.lock()
        let current = handle
        lock.unlock()
        if let current { core.streamCancel(current) }
    }

    /// Releases the handle; safe to call more than once and from any thread.
    public func close() {
        lock.lock()
        let current = handle
        handle = nil
        lock.unlock()
        if let current { core.streamClose(current) }
    }

    deinit { close() }
}

/// Swift bridge to the LingXi Rust core (dlopen + dlsym, no build-time link).
///
/// The core owns provider payload translation, response normalization,
/// incremental streaming, SigV4 signing and embedding/rerank mapping, so the
/// Swift SDK never re-implements protocol behaviour. The shared library is
/// resolved at runtime: `LINGXI_CORE_LIB`, then the `lingxi.core.lib` system
/// property, then `core/target/{debug,release}` walking up from the CWD.
public final class RustCore: @unchecked Sendable {
    private typealias Str0 = @convention(c) () -> UnsafeMutablePointer<CChar>?
    private typealias Str1 = @convention(c) (UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>?
    private typealias Str2 = @convention(c) (UnsafePointer<CChar>, UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>?
    private typealias FreeFn = @convention(c) (UnsafeMutablePointer<CChar>?) -> Void
    private typealias OpenFn = @convention(c) (UnsafePointer<CChar>) -> UnsafeMutableRawPointer?
    private typealias NextFn = @convention(c) (UnsafeMutableRawPointer, UInt64) -> UnsafeMutablePointer<CChar>?
    private typealias HandleOnlyFn = @convention(c) (UnsafeMutableRawPointer) -> Void

    private static let lock = NSLock()
    nonisolated(unsafe) private static var cached: RustCore?
    nonisolated(unsafe) private static var cachedFailure: String?
    nonisolated(unsafe) private static var didAttempt = false

    public let libraryPath: String
    private let image: UnsafeMutableRawPointer
    private let symbols: [String: UnsafeMutableRawPointer]

    /// Loads (once) the core shared library or throws with the reason.
    public static func shared() throws -> RustCore {
        lock.lock()
        defer { lock.unlock() }
        if let cached { return cached }
        if didAttempt, let cachedFailure {
            throw RustCoreError.unavailable(cachedFailure)
        }
        didAttempt = true
        do {
            let core = try RustCore.load()
            cached = core
            return core
        } catch let error as RustCoreError {
            if case .unavailable(let reason) = error { cachedFailure = reason }
            throw error
        }
    }

    public static var isAvailable: Bool {
        (try? shared()) != nil
    }

    private static func load() throws -> RustCore {
        guard let path = locateLibrary() else {
            throw RustCoreError.unavailable(
                "set LINGXI_CORE_LIB or run 'cargo build --manifest-path core/Cargo.toml' first")
        }
        guard let image = dlopen(path, RTLD_NOW | RTLD_LOCAL) else {
            let reason = dlerror().map { String(cString: $0) } ?? "dlopen failed"
            throw RustCoreError.unavailable(reason)
        }
        var symbols: [String: UnsafeMutableRawPointer] = [:]
        let required = [
            "lingxi_lmp_translate_request",
            "lingxi_lmp_normalize_response",
            "lingxi_native_prepare",
            "lingxi_unified_prepare",
            "lingxi_unified_embed",
            "lingxi_unified_rerank",
            "lingxi_stream_open",
            "lingxi_stream_open_error",
            "lingxi_stream_next",
            "lingxi_stream_cancel",
            "lingxi_stream_close",
            "lingxi_string_free",
        ]
        for name in required {
            guard let symbol = dlsym(image, name) else {
                throw RustCoreError.unavailable("lingxi_core is missing \(name); rebuild it with: cargo build --manifest-path core/Cargo.toml")
            }
            symbols[name] = symbol
        }
        return RustCore(libraryPath: path, image: image, symbols: symbols)
    }

    private init(libraryPath: String, image: UnsafeMutableRawPointer, symbols: [String: UnsafeMutableRawPointer]) {
        self.libraryPath = libraryPath
        self.image = image
        self.symbols = symbols
    }

    private static func locateLibrary() -> String? {
        let environment = ProcessInfo.processInfo.environment
        if let explicit = environment["LINGXI_CORE_LIB"], !explicit.isEmpty,
           FileManager.default.fileExists(atPath: explicit) {
            return explicit
        }
        #if os(macOS) || os(iOS) || os(tvOS) || os(watchOS)
        let candidates = ["liblingxi_core.dylib"]
        #elseif os(Windows)
        let candidates = ["lingxi_core.dll"]
        #else
        let candidates = ["liblingxi_core.so"]
        #endif
        var directory = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
        for _ in 0..<8 {
            for candidate in candidates {
                for profile in ["debug", "release"] {
                    let path = directory.appendingPathComponent("core/target/\(profile)/\(candidate)").path
                    if FileManager.default.fileExists(atPath: path) { return path }
                }
            }
            let parent = directory.deletingLastPathComponent()
            if parent.path == directory.path { break }
            directory = parent
        }
        return nil
    }

    // MARK: - Envelope plumbing

    private func symbol<T>(_ name: String, as type: T.Type) throws -> T {
        guard let pointer = symbols[name] else {
            throw RustCoreError.unavailable("lingxi_core is missing \(name)")
        }
        return unsafeBitCast(pointer, to: T.self)
    }

    private func freeResult(_ pointer: UnsafeMutablePointer<CChar>) {
        guard let raw = symbols["lingxi_string_free"] else { return }
        let free = unsafeBitCast(raw, to: FreeFn.self)
        free(pointer)
    }

    private func takeString(_ pointer: UnsafeMutablePointer<CChar>?, _ operation: String) throws -> String {
        guard let pointer else {
            throw RustCoreError.failure(operation: operation, code: "null", message: "the core returned a null pointer")
        }
        let text = String(cString: pointer)
        freeResult(pointer)
        return text
    }

    static func jsonObject(_ text: String) throws -> [String: Any] {
        guard let data = text.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data, options: []),
              let dictionary = object as? [String: Any] else {
            throw RustCoreError.failure(operation: "decode", code: "json", message: "expected a JSON object")
        }
        return dictionary
    }

    private func encode(_ value: Any) throws -> String {
        guard JSONSerialization.isValidJSONObject(value) else {
            throw RustCoreError.failure(operation: "encode", code: "json", message: "value is not JSON serializable")
        }
        let data = try JSONSerialization.data(withJSONObject: value, options: [])
        return String(decoding: data, as: UTF8.self)
    }

    private func requireOK(_ envelope: [String: Any], _ operation: String) throws -> [String: Any] {
        if let ok = envelope["ok"] as? Bool, ok { return envelope }
        let code = envelope["code"] as? String ?? "error"
        let message = envelope["error"] as? String ?? "unknown core failure"
        throw RustCoreError.failure(operation: operation, code: code, message: message)
    }

    private func call0(_ name: String) throws -> String {
        let function = try symbol(name, as: Str0.self)
        return try takeString(function(), name)
    }

    private func call1(_ name: String, _ first: String) throws -> String {
        let function = try symbol(name, as: Str1.self)
        return try first.withCString { pointer in
            try takeString(function(pointer), name)
        }
    }

    private func call2(_ name: String, _ first: String, _ second: String) throws -> String {
        let function = try symbol(name, as: Str2.self)
        return try first.withCString { a in
            try second.withCString { b in
                try takeString(function(a, b), name)
            }
        }
    }

    // MARK: - LMP translation and normalization

    public func translateRequest(protocol proto: String, request: [String: Any]) throws -> [String: Any] {
        let text = try call2("lingxi_lmp_translate_request", proto, try encode(request))
        let envelope = try requireOK(try Self.jsonObject(text), "translate_request")
        return envelope["payload"] as? [String: Any] ?? [:]
    }

    public func normalizeResponse(protocol proto: String, raw: [String: Any]) throws -> [String: Any] {
        let text = try call2("lingxi_lmp_normalize_response", proto, try encode(raw))
        let envelope = try requireOK(try Self.jsonObject(text), "normalize_response")
        return envelope["response"] as? [String: Any] ?? [:]
    }

    public func prepareNative(_ request: [String: Any]) throws -> NativeWireTarget {
        try prepare("lingxi_native_prepare", request)
    }

    public func prepareUnified(_ request: [String: Any]) throws -> NativeWireTarget {
        try prepare("lingxi_unified_prepare", request)
    }

    private func prepare(_ name: String, _ request: [String: Any]) throws -> NativeWireTarget {
        let text = try call1(name, try encode(request))
        let envelope = try requireOK(try Self.jsonObject(text), name)
        let url = envelope["url"] as? String ?? ""
        let framing = envelope["framing"] as? String ?? "sse"
        let headers = (envelope["headers"] as? [String: Any] ?? [:]).compactMapValues { $0 as? String }
        let body = envelope["body"] ?? [:]
        let bodyData = try JSONSerialization.data(withJSONObject: body, options: [])
        return NativeWireTarget(
            url: url,
            headers: headers,
            bodyData: bodyData,
            framing: framing,
            protocolName: envelope["protocol"] as? String
        )
    }

    public func embed(_ request: [String: Any]) throws -> [String: Any] {
        let text = try call1("lingxi_unified_embed", try encode(request))
        let envelope = try requireOK(try Self.jsonObject(text), "unified_embed")
        return envelope["response"] as? [String: Any] ?? [:]
    }

    public func rerank(_ request: [String: Any]) throws -> [String: Any] {
        let text = try call1("lingxi_unified_rerank", try encode(request))
        let envelope = try requireOK(try Self.jsonObject(text), "unified_rerank")
        return envelope["response"] as? [String: Any] ?? [:]
    }

    // MARK: - Stream handles

    func openStream(_ request: [String: Any]) throws -> UnsafeMutableRawPointer {
        let open = try symbol("lingxi_stream_open", as: OpenFn.self)
        let json = try encode(request)
        let handle = json.withCString { open($0) }
        if let handle { return handle }
        let reason: (code: String, message: String)
        if let text = try? call0("lingxi_stream_open_error"),
           let envelope = try? Self.jsonObject(text) {
            reason = (envelope["code"] as? String ?? "error", envelope["error"] as? String ?? "stream open failed")
        } else {
            reason = ("error", "stream open failed")
        }
        throw RustCoreError.failure(operation: "stream_open", code: reason.code, message: reason.message)
    }

    func streamNext(_ handle: UnsafeMutableRawPointer, timeoutMilliseconds: UInt64) throws -> CoreStreamFrame? {
        let next = try symbol("lingxi_stream_next", as: NextFn.self)
        let text = try takeString(next(handle, timeoutMilliseconds), "stream_next")
        let envelope = try Self.jsonObject(text)
        if let ok = envelope["ok"] as? Bool, ok {
            if envelope["done"] as? Bool == true { return nil }
            if let event = envelope["event"] as? [String: Any] {
                return CoreStreamFrame(json: event, done: false)
            }
            return CoreStreamFrame(json: envelope, done: false)
        }
        let code = envelope["code"] as? String ?? "error"
        let message = envelope["error"] as? String ?? "stream failure"
        throw RustCoreError.failure(operation: "stream_next", code: code, message: message)
    }

    func streamCancel(_ handle: UnsafeMutableRawPointer) {
        guard let raw = symbols["lingxi_stream_cancel"] else { return }
        unsafeBitCast(raw, to: HandleOnlyFn.self)(handle)
    }

    func streamClose(_ handle: UnsafeMutableRawPointer) {
        guard let raw = symbols["lingxi_stream_close"] else { return }
        unsafeBitCast(raw, to: HandleOnlyFn.self)(handle)
    }
}
