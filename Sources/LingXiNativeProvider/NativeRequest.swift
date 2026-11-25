import Foundation
import LingXiProviderRegistry

extension NativeClient {
    /// Resolves the final URL, headers and body with the Rust core and POSTs it.
    ///
    /// The caller keeps the provider's native payload and receives the
    /// provider's native response JSON: nothing here normalizes to LMP.
    public func request(
        protocol proto: NativeProtocol,
        cfg: NativeRequestConfig,
        model: String?,
        payload: [String: Any]
    ) async throws -> [String: Any] {
        var request: [String: Any] = [
            "protocol": proto.rawValue,
            "baseUrl": cfg.baseURL,
            "payload": payload,
            "allowInsecureHttp": cfg.allowInsecureHTTP,
            "timeoutMs": Int(cfg.timeout * 1000),
        ]
        if let model { request["model"] = model }
        if let key = cfg.apiKey { request["apiKey"] = key }
        if !cfg.headers.isEmpty { request["headers"] = cfg.headers }
        let target: NativeWireTarget
        do {
            target = try RustCore.shared().prepareNative(request)
        } catch {
            throw Self.mapCoreError(error)
        }
        return try await postJSON(
            url: target.url,
            headers: target.headers,
            payloadData: target.bodyData,
            allowInsecureHTTP: cfg.allowInsecureHTTP
        )
    }
}
