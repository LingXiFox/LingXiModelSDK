package cn.lingxifox.modelsdk;

import java.net.URI;
import java.util.List;
import java.util.Map;

public final class ProviderRegistry {
    public static final List<String> ALL_PROTOCOLS = List.of(
        "openai_chat_completions",
        "openai_responses",
        "anthropic_messages",
        "google_gemini",
        "aws_bedrock",
        "google_vertex_ai",
        "azure_openai",
        "ollama",
        "cohere"
    );

    private final Map<String, Object> providers;

    private ProviderRegistry(Map<String, Object> providers) {
        this.providers = providers;
    }

    @SuppressWarnings("unchecked")
    public static ProviderRegistry fromJson(String jsonText) {
        Object parsed = JsonParser.parse(jsonText);
        if (!(parsed instanceof Map<?, ?> doc)) {
            throw new IllegalArgumentException("Registry root must be a JSON object");
        }
        if (!"1.0".equals(doc.get("schemaVersion"))) {
            throw new IllegalArgumentException("Unsupported schemaVersion: " + doc.get("schemaVersion"));
        }
        Object rawProviders = doc.get("providers");
        if (!(rawProviders instanceof Map<?, ?> map)) {
            throw new IllegalArgumentException("Missing providers map");
        }
        return new ProviderRegistry((Map<String, Object>) map);
    }

    public int size() {
        return providers.size();
    }

    @SuppressWarnings("unchecked")
    public Map<String, Object> get(String providerId) {
        Object v = providers.get(providerId);
        return (v instanceof Map<?, ?> m) ? (Map<String, Object>) m : null;
    }

    @SuppressWarnings("unchecked")
    public String resolveEndpoint(
        String providerId,
        String protocol,
        String endpointOverride,
        boolean allowInsecureHttp
    ) {
        Map<String, Object> entry = get(providerId);
        if (entry == null) {
            throw new IllegalArgumentException("Unknown provider: " + providerId);
        }
        List<Object> protos = (List<Object>) entry.get("protocols");
        if (protos == null || !protos.contains(protocol)) {
            throw new IllegalArgumentException("Provider '" + providerId + "' does not declare protocol '" + protocol + "'");
        }
        String base = endpointOverride;
        if (base == null || base.isBlank()) {
            Map<String, Object> endpoints = (Map<String, Object>) entry.get("officialEndpoints");
            base = endpoints != null ? (String) endpoints.get(protocol) : null;
        }
        if (base == null || base.isBlank()) {
            throw new IllegalArgumentException("No endpoint for " + providerId + "/" + protocol);
        }
        String clean = base.replaceAll("/+$", "");
        validateEndpointUrl(clean, allowInsecureHttp);
        return clean;
    }

    public static void validateEndpointUrl(String rawUrl, boolean allowInsecureHttp) {
        URI uri = URI.create(rawUrl);
        String scheme = uri.getScheme() == null ? "" : uri.getScheme().toLowerCase();
        String host = uri.getHost() == null ? "" : uri.getHost().toLowerCase();
        if ("https".equals(scheme) && !host.isEmpty()) {
            return;
        }
        if ("http".equals(scheme) && !host.isEmpty()) {
            if (allowInsecureHttp || "127.0.0.1".equals(host) || "localhost".equals(host) || "::1".equals(host)) {
                return;
            }
            throw new IllegalArgumentException("Non-HTTPS endpoint disallowed: " + rawUrl);
        }
        throw new IllegalArgumentException("Unsupported endpoint URL: " + rawUrl);
    }
}
