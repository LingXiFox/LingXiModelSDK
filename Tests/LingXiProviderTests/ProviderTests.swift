import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
import Testing
@testable import LingXiProviderRegistry
@testable import LingXiNativeProvider
@testable import LingXiUnifiedProvider

final class MockURLProtocol: URLProtocol, @unchecked Sendable {
    nonisolated(unsafe) static var handler: ((URLRequest) throws -> (HTTPURLResponse, Data))?

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        guard let handler = Self.handler else { return }
        do {
            let (resp, data) = try handler(request)
            client?.urlProtocol(self, didReceive: resp, cacheStoragePolicy: .notAllowed)
            client?.urlProtocol(self, didLoad: data)
            client?.urlProtocolDidFinishLoading(self)
        } catch {
            client?.urlProtocol(self, didFailWithError: error)
        }
    }

    override func stopLoading() {}
}

@Test func registryAndSigV4AndUnifiedProtocols() async throws {
    let regJSON = """
    {"schemaVersion":"1.0","providers":{"amazon-bedrock":{"id":"amazon-bedrock","name":"Amazon Bedrock","protocols":["aws_bedrock"],"officialEndpoints":{"aws_bedrock":"https://bedrock-runtime.us-east-1.amazonaws.com"},"auth":{"type":"aws_sigv4"},"verificationStatus":"mock_tested"}}}
    """.data(using: .utf8)!
    let reg = try ProviderRegistry.decoded(from: regJSON)
    let resolved = try reg.resolveEndpoint(providerID: "amazon-bedrock", protocol: .awsBedrock)
    #expect(resolved.baseURL == "https://bedrock-runtime.us-east-1.amazonaws.com")

    let config = URLSessionConfiguration.ephemeral
    config.protocolClasses = [MockURLProtocol.self]
    let native = NativeClient(configuration: config)
    let unified = UnifiedClient(native: native)

    MockURLProtocol.handler = { req in
        let auth = req.value(forHTTPHeaderField: "Authorization") ?? ""
        #expect(auth.hasPrefix("AWS4-HMAC-SHA256"))
        let json = #"{"output":{"message":{"role":"assistant","content":[{"reasoningContent":{"reasoningText":{"text":"t","signature":"sig-swift"}}},{"text":"hello"}]}},"stopReason":"end_turn","usage":{"inputTokens":5,"outputTokens":3,"totalTokens":8}}"#
        let resp = HTTPURLResponse(url: req.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!
        return (resp, Data(json.utf8))
    }

    let out = try await unified.complete(
        protocol: .awsBedrock,
        baseURL: "https://bedrock-runtime.us-east-1.amazonaws.com",
        apiKey: "AKIA:SECRET",
        request: LmpRequest(model: "anthropic.claude-3-5-sonnet", messages: [LmpMessage(role: "user", content: [.text("hi")])])
    )
    #expect(out.content.contains(.thinking("t", signature: "sig-swift")))

    await #expect(throws: LmpError.self) {
        _ = try await unified.rerank(
            protocol: .openaiChatCompletions,
            baseURL: "https://api.openai.com/v1",
            apiKey: "sk-test",
            model: "m",
            query: "q",
            documents: ["a"]
        )
    }
}
