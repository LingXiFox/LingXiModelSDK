package cn.lingxifox.modelsdk;

import java.util.List;
import java.util.Map;

public final class UnifiedClient {
    public static final class UnsupportedFeatureException extends RuntimeException {
        public UnsupportedFeatureException(String protocol, String feature) {
            super("Unsupported feature '" + feature + "' for protocol '" + protocol + "'");
        }
    }

    private final NativeClient nativeClient;

    public UnifiedClient() {
        this(new NativeClient());
    }

    public UnifiedClient(NativeClient nativeClient) {
        this.nativeClient = nativeClient;
    }

    public Map<String, Object> completeText(
        String protocol,
        String baseUrl,
        String apiKey,
        String model,
        String prompt
    ) throws Exception {
        String cleanBase = baseUrl.replaceAll("/+$", "");
        String uri;
        String payload;
        String escPrompt = prompt.replace("\"", "\\\"");
        String escModel = model.replace("\"", "\\\"");

        switch (protocol) {
            case "openai_chat_completions" -> {
                uri = "/chat/completions";
                payload = "{\"model\":\"" + escModel + "\",\"messages\":[{\"role\":\"user\",\"content\":\"" + escPrompt + "\"}]}";
            }
            case "openai_responses" -> {
                uri = "/responses";
                payload = "{\"model\":\"" + escModel + "\",\"input\":[{\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"" + escPrompt + "\"}]}]}";
            }
            case "anthropic_messages" -> {
                uri = cleanBase.endsWith("/v1") ? "/messages" : "/v1/messages";
                payload = "{\"model\":\"" + escModel + "\",\"max_tokens\":1024,\"messages\":[{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"" + escPrompt + "\"}]}]}";
            }
            case "google_gemini" -> {
                uri = "/v1beta/models/" + escModel + ":generateContent";
                payload = "{\"contents\":[{\"role\":\"user\",\"parts\":[{\"text\":\"" + escPrompt + "\"}]}]}";
            }
            case "aws_bedrock" -> {
                uri = "/model/" + escModel + "/converse";
                payload = "{\"messages\":[{\"role\":\"user\",\"content\":[{\"text\":\"" + escPrompt + "\"}]}]}";
            }
            case "google_vertex_ai" -> {
                uri = "/v1/publishers/google/models/" + escModel + ":generateContent";
                payload = "{\"contents\":[{\"role\":\"user\",\"parts\":[{\"text\":\"" + escPrompt + "\"}]}]}";
            }
            case "azure_openai" -> {
                uri = "/openai/deployments/" + escModel + "/chat/completions?api-version=2024-10-21";
                payload = "{\"model\":\"" + escModel + "\",\"messages\":[{\"role\":\"user\",\"content\":\"" + escPrompt + "\"}]}";
            }
            case "ollama" -> {
                uri = "/api/chat";
                payload = "{\"model\":\"" + escModel + "\",\"stream\":false,\"messages\":[{\"role\":\"user\",\"content\":\"" + escPrompt + "\"}]}";
            }
            case "cohere" -> {
                uri = "/v2/chat";
                payload = "{\"model\":\"" + escModel + "\",\"messages\":[{\"role\":\"user\",\"content\":\"" + escPrompt + "\"}]}";
            }
            default -> throw new UnsupportedFeatureException(protocol, "protocol");
        }

        Map<String, String> hdrs = nativeClient.buildAuthHeaders(protocol, cleanBase, apiKey, uri, payload);
        return nativeClient.postJson(cleanBase + uri, hdrs, payload, false);
    }

    public Map<String, Object> rerank(
        String protocol,
        String baseUrl,
        String apiKey,
        String model,
        String query,
        List<String> documents
    ) throws Exception {
        if (!"cohere".equals(protocol)) {
            throw new UnsupportedFeatureException(protocol, "rerank");
        }
        StringBuilder docsJson = new StringBuilder("[");
        for (int i = 0; i < documents.size(); i++) {
            if (i > 0) docsJson.append(',');
            docsJson.append('"').append(documents.get(i).replace("\"", "\\\"")).append('"');
        }
        docsJson.append(']');
        String payload = "{\"model\":\"" + model + "\",\"query\":\"" + query + "\",\"documents\":" + docsJson + "}";
        return nativeClient.postJson(baseUrl.replaceAll("/+$", "") + "/v2/rerank",
            Map.of("Authorization", "Bearer " + apiKey), payload, false);
    }
}
