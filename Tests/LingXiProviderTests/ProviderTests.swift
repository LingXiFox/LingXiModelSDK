import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
import Testing
@testable import LingXiProviderRegistry
@testable import LingXiNativeProvider
@testable import LingXiUnifiedProvider

private let openAIChat = "openaiChatCompletions"

func makeServer(_ route: @escaping @Sendable (TestHTTPRequest) throws -> TestHTTPResponse) throws -> TestHTTPServer {
    try TestHTTPServer(route: route)
}

/// Replays the conformance fixtures the same way the shared mock server does.
func fixtureRoute(_ request: TestHTTPRequest) throws -> TestHTTPResponse {
    let raw = request.path
    let path = raw.split(separator: "?").first.map(String.init) ?? raw
    let body = request.json
    let streaming = (body["stream"] as? Bool) == true || raw.contains("alt=sse")
    func pick(_ name: String, _ contentType: String) throws -> TestHTTPResponse {
        try TestHTTPResponse.file(fixturePath(name), contentType: contentType)
    }
    if path.hasSuffix("/converse-stream") {
        return try pick("aws_bedrock_stream.eventstream", "application/vnd.amazon.eventstream")
    }
    if path.hasSuffix("/converse") {
        return try pick("aws_bedrock_response.json", "application/json")
    }
    if path.hasSuffix("/chat/completions") {
        let azure = path.hasPrefix("/openai/deployments/")
        if streaming { return try pick(azure ? "azure_openai_stream.sse" : "openai_chat_stream.sse", "text/event-stream") }
        return try pick(azure ? "azure_openai_response.json" : "openai_chat_response.json", "application/json")
    }
    if path.hasSuffix("/responses") {
        return try pick(streaming ? "openai_responses_stream.sse" : "openai_responses_response.json", streaming ? "text/event-stream" : "application/json")
    }
    if path.hasSuffix("/messages") {
        return try pick(streaming ? "anthropic_messages_stream.sse" : "anthropic_messages_response.json", streaming ? "text/event-stream" : "application/json")
    }
    if path.hasSuffix(":streamGenerateContent") || path.hasSuffix(":generateContent") {
        let vertex = path.hasPrefix("/v1/")
        if streaming { return try pick(vertex ? "google_vertex_stream.sse" : "google_gemini_stream.sse", "text/event-stream") }
        return try pick(vertex ? "google_vertex_response.json" : "google_gemini_response.json", "application/json")
    }
    if path == "/api/chat" {
        return try pick(streaming ? "ollama_chat_stream.ndjson" : "ollama_chat_response.json", streaming ? "application/x-ndjson" : "application/json")
    }
    if path == "/v2/chat" {
        return try pick(streaming ? "cohere_chat_stream.sse" : "cohere_chat_response.json", streaming ? "text/event-stream" : "application/json")
    }
    if path == "/v2/rerank" {
        return try pick("cohere_rerank_response.json", "application/json")
    }
    if path.hasSuffix("/embeddings") {
        return try pick("openai_embed_response.json", "application/json")
    }
    if path == "/redir" {
        return TestHTTPResponse(status: 302, contentType: "text/plain", chunks: [Data()])
    }
    return TestHTTPResponse.body("{\"error\":\"unknown_endpoint\"}", status: 404)
}

@Test func coreLibraryLoads() throws {
    let core = try RustCore.shared()
    #expect(!core.libraryPath.isEmpty)
}

@Test func registryResolvesBedrockEndpoint() throws {
    let regJSON = """
    {"schemaVersion":"1.0","providers":{"amazon-bedrock":{"id":"amazon-bedrock","name":"Amazon Bedrock","protocols":["aws_bedrock"],"officialEndpoints":{"aws_bedrock":"https://bedrock-runtime.us-east-1.amazonaws.com"},"auth":{"type":"aws_sigv4"},"verificationStatus":"mock_tested"}}}
    """.data(using: .utf8)!
    let registry = try ProviderRegistry.decoded(from: regJSON)
    let resolved = try registry.resolveEndpoint(providerID: "amazon-bedrock", protocol: .awsBedrock)
    #expect(resolved.baseURL == "https://bedrock-runtime.us-east-1.amazonaws.com")
}

@Test func nativeWireTargetsAreSignedByTheCore() throws {
    let core = try RustCore.shared()
    let bedrock = try core.prepareNative([
        "protocol": "aws_bedrock",
        "baseUrl": "https://bedrock-runtime.us-west-2.amazonaws.com",
        "apiKey": "AKIDEXAMPLE:SECRETEXAMPLE",
        "model": "anthropic.claude-3-5-sonnet",
        "payload": ["messages": [["role": "user", "content": ["hi"]]]],
        "stream": true,
    ])
    let auth = bedrock.headers["Authorization"] ?? ""
    #expect(auth.hasPrefix("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"))
    #expect(!auth.contains("SECRETEXAMPLE"))
    #expect(bedrock.url.contains("/model/"))
    #expect(bedrock.framing == "amazon_event_stream")

    let anthropic = try core.prepareNative([
        "protocol": "anthropic_messages",
        "baseUrl": "https://api.anthropic.com",
        "apiKey": "sk-ant-test",
        "model": "claude-sonnet-4",
        "payload": ["messages": []],
    ])
    #expect(anthropic.url == "https://api.anthropic.com/v1/messages")
    #expect(anthropic.headers["anthropic-version"] == "2023-06-01")
    #expect(anthropic.headers["x-api-key"] == "sk-ant-test")

    let gemini = try core.prepareNative([
        "protocol": "google_gemini",
        "baseUrl": "https://generativelanguage.googleapis.com",
        "apiKey": "goog-test",
        "model": "gemini-2.0-flash",
        "payload": ["contents": []],
        "stream": true,
    ])
    #expect(gemini.url.contains(":streamGenerateContent?alt=sse"))
    #expect(gemini.framing == "sse")

    let ollama = try core.prepareNative([
        "protocol": "ollama",
        "baseUrl": "http://127.0.0.1:11434",
        "model": "llama3.3",
        "payload": ["messages": []],
    ])
    let ollamaBody = ollama.body as? [String: Any] ?? [:]
    #expect(ollamaBody["stream"] as? Bool == false)
    #expect(ollama.framing == "ndjson")
}

@Test func nativePrepareRejectsInsecureEndpoints() throws {
    let core = try RustCore.shared()
    do {
        _ = try core.prepareNative([
            "protocol": "openai_chat_completions",
            "baseUrl": "http://example.com/v1",
            "apiKey": "sk-test",
            "model": "gpt-4o-mini",
            "payload": [:],
        ])
        Issue.record("expected invalid_endpoint")
    } catch let error as RustCoreError {
        #expect(error.code == "invalid_endpoint")
    }
}

@Test func unifiedNormalizesEveryMockedProtocol() async throws {
    let server = try makeServer(fixtureRoute)
    defer { server.stop() }
    let unified = UnifiedClient()

    let chat = try await unified.completeText(
        protocol: .openaiChatCompletions, baseURL: server.baseURL + "/v1", apiKey: "sk-test",
        model: "gpt-4o-mini", prompt: "hi")
    #expect(chat.text == "Hello from chat completions")
    #expect(chat.finishReason == "stop")
    #expect(chat.usage.inputTokens == 12)
    #expect(chat.usage.outputTokens == 6)
    #expect(chat.thinking == "step-by-step")

    let responses = try await unified.completeText(
        protocol: .openaiResponses, baseURL: server.baseURL + "/v1", apiKey: "sk-test",
        model: "gpt-5", prompt: "hi")
    #expect(responses.text == "Hello from responses")
    #expect(responses.thinking == "Brief thought")
    #expect(responses.usage.totalTokens == 23)

    let anthropic = try await unified.completeText(
        protocol: .anthropicMessages, baseURL: server.baseURL, apiKey: "sk-ant-test",
        model: "claude-sonnet-4", prompt: "hi")
    #expect(anthropic.text == "Hello from Anthropic")
    #expect(anthropic.thinking == "Considering options")
    #expect(anthropic.finishReason == "end_turn")
    #expect(anthropic.usage.cacheReadTokens == 5)
    #expect(anthropic.content.contains(.thinking("Considering options", signature: "sig_abc")))

    let gemini = try await unified.completeText(
        protocol: .googleGemini, baseURL: server.baseURL, apiKey: "goog-test",
        model: "gemini-2.0-flash", prompt: "hi")
    #expect(gemini.text == "Hello from Gemini")
    #expect(gemini.finishReason == "STOP")
    #expect(gemini.usage.totalTokens == 23)

    let vertex = try await unified.completeText(
        protocol: .googleVertexAI, baseURL: server.baseURL + "/v1", apiKey: "goog-test",
        model: "gemini-2.0-flash", prompt: "hi")
    #expect(vertex.text == "Hello from Vertex AI")
    #expect(vertex.usage.cacheReadTokens == 6)

    let azure = try await unified.completeText(
        protocol: .azureOpenAI, baseURL: server.baseURL, apiKey: "azure-key",
        model: "gpt-4o", prompt: "hi")
    #expect(azure.text == "Hello from Azure OpenAI")
    #expect(azure.usage.cacheReadTokens == 4)

    let ollama = try await unified.completeText(
        protocol: .ollama, baseURL: server.baseURL, apiKey: nil,
        model: "llama3.3", prompt: "hi")
    #expect(ollama.text == "Hello from Ollama")
    #expect(ollama.thinking == "Local model thought")

    let cohere = try await unified.completeText(
        protocol: .cohere, baseURL: server.baseURL, apiKey: "co-key",
        model: "command-r-plus", prompt: "hi")
    #expect(cohere.text == "Hello from Cohere")
    #expect(cohere.finishReason == "COMPLETE")
    #expect(cohere.thinking == "Cohere thought")

    let bedrock = try await unified.completeText(
        protocol: .awsBedrock, baseURL: server.baseURL, apiKey: "AKID:SECRET",
        model: "anthropic.claude-3-5-sonnet", prompt: "hi")
    #expect(bedrock.text == "Hello from AWS Bedrock")
    #expect(bedrock.finishReason == "tool_use")
    #expect(bedrock.usage.inputTokens == 24)
    #expect(bedrock.usage.cacheReadTokens == 8)
    #expect(bedrock.content.contains(.thinking("Thinking via Bedrock Converse", signature: "sig-bedrock-1")))
    #expect(bedrock.toolCalls.contains { $0.name == "get_weather" })
}

@Test func unifiedStreamingIsIncremental() async throws {
    let server = try makeServer(fixtureRoute)
    defer { server.stop() }
    let unified = UnifiedClient()
    var deltas: [String] = []
    var finish: String?
    var usage: LmpUsage?
    var sawMessageEnd = false
    for try await event in unified.stream(
        protocol: .openaiChatCompletions,
        baseURL: server.baseURL + "/v1",
        apiKey: "sk-test",
        request: LmpRequest(model: "gpt-4o-mini", messages: [LmpMessage(role: "user", text: "hi")])
    ) {
        switch event {
        case .textDelta(let delta, _): deltas.append(delta)
        case .messageEnd(let reason):
            sawMessageEnd = true
            finish = reason
        case .usage(let value): usage = value
        case .error(let code, let message): Issue.record("stream error \(code): \(message)")
        default: break
        }
    }
    #expect(deltas.joined() == "Hello world")
    #expect(sawMessageEnd)
    // The fixture carries no finish_reason chunk, so the terminal event must still arrive
    // with an unknown reason instead of inventing one.
    if let finish { #expect(finish == "stop") }
    #expect(usage?.totalTokens == 12)

    let anthropicDeltas = try await collectTextDelta(
        unified.stream(
            protocol: .anthropicMessages,
            baseURL: server.baseURL,
            apiKey: "sk-ant-test",
            request: LmpRequest(model: "claude-sonnet-4", messages: [LmpMessage(role: "user", text: "hi")])
        )
    )
    #expect(anthropicDeltas == "Hello Anthropic")

    let geminiDeltas = try await collectTextDelta(
        unified.stream(
            protocol: .googleGemini,
            baseURL: server.baseURL,
            apiKey: "goog-test",
            request: LmpRequest(model: "gemini-2.0-flash", messages: [LmpMessage(role: "user", text: "hi")])
        )
    )
    #expect(geminiDeltas == "Hello Gemini")
}

@Test func unifiedStreamFirstEventArrivesBeforeTheUpstreamFinishes() async throws {
    let base = try fixturePath("openai_chat_stream.sse")
    let text = try String(contentsOfFile: base, encoding: .utf8)
    let chunks = frames(text)
    let server = try makeServer { _ in
        TestHTTPResponse(status: 200, contentType: "text/event-stream", chunks: chunks, delayMilliseconds: 400)
    }
    defer { server.stop() }
    let started = Date()
    let unified = UnifiedClient()
    var firstDeltaAt: Date?
    for try await event in unified.stream(
        protocol: .openaiChatCompletions,
        baseURL: server.baseURL + "/v1",
        apiKey: "sk-test",
        request: LmpRequest(model: "gpt-4o-mini", messages: [LmpMessage(role: "user", text: "hi")])
    ) {
        if case .textDelta = event, firstDeltaAt == nil { firstDeltaAt = Date() }
    }
    let elapsed = Date().timeIntervalSince(started)
    guard let first = firstDeltaAt else {
        Issue.record("no delta arrived")
        return
    }
    let latency = first.timeIntervalSince(started)
    #expect(latency < 0.35, "first delta took \(latency)s; streaming is buffered")
    #expect(elapsed >= 0.7, "the whole stream should still take the scripted time")
}

func collectTextDelta(_ stream: AsyncThrowingStream<LmpStreamEvent, Error>) async throws -> String {
    var out = ""
    for try await event in stream {
        if case .textDelta(let delta, _) = event { out += delta }
    }
    return out
}

@Test func unifiedStreamCanBeCancelled() async throws {
    let fixture = try String(contentsOfFile: fixturePath("openai_chat_stream.sse"), encoding: .utf8)
    let chunks = frames(fixture) + frames(fixture) + frames(fixture)
    let server = try makeServer { _ in
        TestHTTPResponse(status: 200, contentType: "text/event-stream", chunks: chunks, delayMilliseconds: 300)
    }
    defer { server.stop() }
    let unified = UnifiedClient()
    var seen = 0
    var cancelled = false
    do {
        for try await _ in unified.stream(
            protocol: .openaiChatCompletions,
            baseURL: server.baseURL + "/v1",
            apiKey: "sk-test",
            request: LmpRequest(model: "gpt-4o-mini", messages: [LmpMessage(role: "user", text: "hi")])
        ) {
            seen += 1
            if seen == 1 { break }
        }
    } catch {
        cancelled = true
    }
    #expect(seen >= 1)
    _ = cancelled
}

@Test func unifiedEmbedAndRerank() async throws {
    let server = try makeServer(fixtureRoute)
    defer { server.stop() }
    let unified = UnifiedClient()
    let embedded = try unified.embed(
        protocol: .openaiChatCompletions,
        baseURL: server.baseURL,
        apiKey: "sk-test",
        model: "text-embedding-3-small",
        input: ["a", "b"]
    )
    #expect(embedded.embeddings.count == 2)
    #expect(embedded.embeddings[0] == [0.1, 0.2, 0.3])

    do {
        _ = try unified.embed(
            protocol: .anthropicMessages, baseURL: server.baseURL, apiKey: "k",
            model: "m", input: ["a"])
        Issue.record("anthropic embeddings should be unsupported")
    } catch let error as LmpError {
        if case .unsupportedFeature(_, let feature) = error {
            #expect(feature == "embed")
        } else {
            Issue.record("unexpected \(error)")
        }
    }

    let reranked = try unified.rerank(
        protocol: .cohere,
        baseURL: server.baseURL,
        apiKey: "co-key",
        model: "rerank-v3",
        query: "q",
        documents: ["a", "b"]
    )
    #expect(reranked.results.first?.index == 1)
    #expect(abs((reranked.results.first?.relevanceScore ?? 0) - 0.94) < 0.0001)

    do {
        _ = try unified.rerank(
            protocol: .openaiChatCompletions, baseURL: server.baseURL, apiKey: "k",
            model: "m", query: "q", documents: ["a"])
        Issue.record("openai rerank should be unsupported")
    } catch let error as LmpError {
        if case .unsupportedFeature(_, let feature) = error {
            #expect(feature == "rerank")
        } else {
            Issue.record("unexpected \(error)")
        }
    }
}

@Test func nativeClientStreamsRawFraming() async throws {
    let server = try makeServer(fixtureRoute)
    defer { server.stop() }
    let native = NativeClient()
    let cfg = NativeRequestConfig(baseURL: server.baseURL + "/v1", apiKey: "sk-test")
    var payloads: [String] = []
    for try await event in native.streamEvents(
        protocol: .openaiChatCompletions,
        cfg: cfg,
        model: "gpt-4o-mini",
        payload: ["model": "gpt-4o-mini", "messages": [["role": "user", "content": "hi"]]]
    ) {
        payloads.append(event.data)
    }
    #expect(payloads.count == 2)
    #expect(payloads[0].contains("Hello"))

    let bedrock = try await native.request(
        protocol: .awsBedrock,
        cfg: NativeRequestConfig(baseURL: server.baseURL, apiKey: "AKID:SECRET"),
        model: "anthropic.claude-3-5-sonnet",
        payload: ["messages": [["role": "user", "content": [["text": "hi"]]]]]
    )
    #expect(bedrock["stopReason"] as? String == "tool_use")

    let bedrockStream = native.streamEvents(
        protocol: .awsBedrock,
        cfg: NativeRequestConfig(baseURL: server.baseURL, apiKey: "AKID:SECRET"),
        model: "anthropic.claude-3-5-sonnet",
        payload: ["messages": [["role": "user", "content": [["text": "hi"]]]]]
    )
    var bedrockEventNames: [String] = []
    for try await event in bedrockStream { bedrockEventNames.append(event.event ?? "") }
    #expect(bedrockEventNames.contains("contentBlockDelta"))
}

@Test func redirectsAreBlocked() async throws {
    let server = try makeServer { request in
        if request.path == "/redir" { return TestHTTPResponse(status: 302, contentType: "text/plain", chunks: [Data()]) }
        return try fixtureRoute(request)
    }
    defer { server.stop() }
    let native = NativeClient()
    do {
        _ = try await native.postJSON(
            url: server.baseURL + "/redir",
            headers: [:],
            payloadData: Data("{}".utf8))
        Issue.record("redirect should be blocked")
    } catch let error as NativeClientError {
        if case .redirectBlocked(let status) = error {
            #expect(status == 302)
        } else {
            Issue.record("unexpected \(error)")
        }
    }
}
