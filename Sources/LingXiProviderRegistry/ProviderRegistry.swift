import Foundation

public enum NativeProtocol: String, Codable, Sendable, CaseIterable {
    case openaiChatCompletions = "openai_chat_completions"
    case openaiResponses = "openai_responses"
    case anthropicMessages = "anthropic_messages"
    case googleGemini = "google_gemini"
    case awsBedrock = "aws_bedrock"
    case googleVertexAI = "google_vertex_ai"
    case azureOpenAI = "azure_openai"
    case ollama = "ollama"
    case cohere = "cohere"
}

public struct AuthConfig: Codable, Sendable {
    public let type: String
    public let headerName: String?
    public let prefix: String?
    public let queryParam: String?
    public let env: [String]?
}

public struct ModelOverride: Codable, Sendable {
    public let endpoint: String?
    public let defaultHeaders: [String: String]?
}

public struct ProviderEntry: Codable, Sendable {
    public let id: String
    public let name: String
    public let protocols: [String]
    public let officialEndpoints: [String: String]
    public let auth: AuthConfig
    public let defaultHeaders: [String: String]?
    public let modelOverrides: [String: ModelOverride]?
    public let verificationStatus: String
}

public enum RegistryError: Error, LocalizedError, Sendable {
    case invalidSchema(String)
    case unknownProvider(String)
    case unsupportedProtocol(provider: String, protocol: String)
    case invalidEndpoint(String)

    public var errorDescription: String? {
        switch self {
        case .invalidSchema(let msg): return "Invalid registry schema: \(msg)"
        case .unknownProvider(let id): return "Unknown provider '\(id)'"
        case .unsupportedProtocol(let p, let proto): return "Provider '\(p)' does not declare protocol '\(proto)'"
        case .invalidEndpoint(let msg): return "Invalid endpoint: \(msg)"
        }
    }
}

public func validateEndpointURL(_ rawURL: String, allowInsecureHTTP: Bool = false) throws {
    guard let url = URL(string: rawURL), let scheme = url.scheme?.lowercased(), let host = url.host?.lowercased() else {
        throw RegistryError.invalidEndpoint("malformed URL '\(rawURL)'")
    }
    if scheme == "https" { return }
    if scheme == "http" {
        let isLoopback = host == "127.0.0.1" || host == "localhost" || host == "::1" || host == "[::1]"
        if isLoopback || allowInsecureHTTP { return }
        throw RegistryError.invalidEndpoint("non-HTTPS endpoint '\(rawURL)' disallowed")
    }
    throw RegistryError.invalidEndpoint("unsupported scheme '\(scheme)'")
}

public struct ProviderRegistry: Sendable {
    private struct Document: Codable {
        let schemaVersion: String
        let providers: [String: ProviderEntry]
    }

    public let providers: [String: ProviderEntry]

    public init(providers: [String: ProviderEntry]) {
        self.providers = providers
    }

    public static func decoded(from data: Data) throws -> ProviderRegistry {
        let doc = try JSONDecoder().decode(Document.self, from: data)
        guard doc.schemaVersion == "1.0" else {
            throw RegistryError.invalidSchema(doc.schemaVersion)
        }
        return ProviderRegistry(providers: doc.providers)
    }

    public func provider(id: String) -> ProviderEntry? {
        providers[id]
    }

    public func resolveEndpoint(
        providerID: String,
        protocol proto: NativeProtocol,
        modelID: String? = nil,
        endpointOverride: String? = nil,
        allowInsecureHTTP: Bool = false
    ) throws -> (entry: ProviderEntry, baseURL: String) {
        guard let entry = providers[providerID] else {
            throw RegistryError.unknownProvider(providerID)
        }
        guard entry.protocols.contains(proto.rawValue) else {
            throw RegistryError.unsupportedProtocol(provider: providerID, protocol: proto.rawValue)
        }
        let modelBase = modelID.flatMap { entry.modelOverrides?[$0]?.endpoint }
        guard let raw = endpointOverride ?? modelBase ?? entry.officialEndpoints[proto.rawValue] else {
            throw RegistryError.invalidEndpoint("no endpoint configured for \(providerID)/\(proto.rawValue)")
        }
        let clean = raw.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        try validateEndpointURL(clean, allowInsecureHTTP: allowInsecureHTTP)
        return (entry, clean)
    }
}
