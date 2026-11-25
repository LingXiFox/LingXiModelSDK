package cn.lingxifox.modelsdk;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * LingXi Unified client: LMP requests in, LMP responses out.
 *
 * <p>All protocol knowledge (payload translation, endpoint joining, provider
 * auth, AWS SigV4, response and stream normalization) lives in the Rust core.
 * This class only moves JSON across the C ABI and performs the HTTP call with
 * the host transport for non-streaming requests. Streaming is handled entirely
 * by the core so tokens arrive incrementally.
 *
 * <p>Capabilities a provider cannot express are reported as
 * {@link UnsupportedFeatureException} (core code {@code unsupported_feature});
 * nothing is silently downgraded.
 */
public final class UnifiedClient {
    public static final class UnsupportedFeatureException extends RuntimeException {
        private final String protocol;
        private final String feature;

        public UnsupportedFeatureException(String protocol, String feature) {
            super("Unsupported feature '" + feature + "' for protocol '" + protocol + "'");
            this.protocol = protocol;
            this.feature = feature;
        }

        public UnsupportedFeatureException(String message, String protocol, String feature) {
            super(message);
            this.protocol = protocol;
            this.feature = feature;
        }

        public String protocol() {
            return protocol;
        }

        public String feature() {
            return feature;
        }
    }

    private final NativeClient nativeClient;
    private final RustCoreBridge bridge;

    public UnifiedClient() {
        this(new NativeClient());
    }

    public UnifiedClient(NativeClient nativeClient) {
        this.nativeClient = nativeClient;
        this.bridge = RustCoreBridge.shared();
    }

    /** True when the Rust core (and therefore this client) is usable. */
    public static boolean available() {
        return RustCoreBridge.available();
    }

    /** Minimal LMP text request for a single user prompt. */
    public static Map<String, Object> textRequest(String model, String prompt) {
        Map<String, Object> message = new LinkedHashMap<>();
        message.put("role", "user");
        message.put("content", List.of(Map.of("type", "text", "text", prompt)));
        Map<String, Object> request = new LinkedHashMap<>();
        request.put("model", model);
        request.put("messages", List.of(message));
        return request;
    }

    /** Non-streaming completion. {@code request} is an LMP request map. */
    public Map<String, Object> complete(
        String protocol,
        String baseUrl,
        String apiKey,
        Map<String, Object> request
    ) throws Exception {
        Map<String, Object> prepareRequest = new LinkedHashMap<>();
        prepareRequest.put("protocol", protocol);
        prepareRequest.put("baseUrl", baseUrl);
        if (apiKey != null) {
            prepareRequest.put("apiKey", apiKey);
        }
        prepareRequest.put("request", request);
        Map<String, Object> target = call(() -> bridge.prepareUnified(prepareRequest), protocol);

        @SuppressWarnings("unchecked")
        Map<String, Object> headers = (Map<String, Object>) target.get("headers");
        Map<String, String> flat = new LinkedHashMap<>();
        if (headers != null) {
            for (Map.Entry<String, Object> entry : headers.entrySet()) {
                flat.put(entry.getKey(), String.valueOf(entry.getValue()));
            }
        }
        String url = String.valueOf(target.get("url"));
        String body = JsonWriter.write(target.get("body"));
        Map<String, Object> raw = nativeClient.postJson(url, flat, body, false);
        return call(() -> bridge.normalizeResponse(protocol, raw), protocol);
    }

    /** Convenience single-prompt completion returning the LMP response map. */
    public Map<String, Object> completeText(
        String protocol,
        String baseUrl,
        String apiKey,
        String model,
        String prompt
    ) throws Exception {
        return complete(protocol, baseUrl, apiKey, textRequest(model, prompt));
    }

    /**
     * Opens an incremental Unified stream. Events are LMP stream events
     * ({@code text_delta}, {@code tool_call_delta}, {@code usage},
     * {@code message_end}), delivered while the provider response is still open.
     */
    public StreamSession stream(
        String protocol,
        String baseUrl,
        String apiKey,
        Map<String, Object> request
    ) {
        Map<String, Object> open = new LinkedHashMap<>();
        open.put("protocol", protocol);
        open.put("baseUrl", baseUrl);
        if (apiKey != null) {
            open.put("apiKey", apiKey);
        }
        open.put("payload", request);
        return StreamSession.open(open, true);
    }

    /** Buffered streaming, for callers that deliberately want the whole list. */
    public List<Map<String, Object>> streamAll(
        String protocol,
        String baseUrl,
        String apiKey,
        Map<String, Object> request
    ) {
        try (StreamSession session = stream(protocol, baseUrl, apiKey, request)) {
            return session.collect();
        }
    }

    /** Normalized embeddings. */
    public Map<String, Object> embed(
        String protocol,
        String baseUrl,
        String apiKey,
        String model,
        List<String> input
    ) {
        Map<String, Object> request = new LinkedHashMap<>();
        request.put("protocol", protocol);
        request.put("baseUrl", baseUrl);
        if (apiKey != null) {
            request.put("apiKey", apiKey);
        }
        request.put("model", model);
        request.put("input", input);
        return call(() -> bridge.embed(request), protocol);
    }

    /** Normalized reranking (Cohere only today; other protocols raise). */
    public Map<String, Object> rerank(
        String protocol,
        String baseUrl,
        String apiKey,
        String model,
        String query,
        List<String> documents
    ) {
        Map<String, Object> request = new LinkedHashMap<>();
        request.put("protocol", protocol);
        request.put("baseUrl", baseUrl);
        if (apiKey != null) {
            request.put("apiKey", apiKey);
        }
        request.put("model", model);
        request.put("query", query);
        request.put("documents", new ArrayList<>(documents));
        return call(() -> bridge.rerank(request), protocol);
    }

    private interface CoreCall {
        Map<String, Object> run();
    }

    /** Maps core error codes onto the SDK exception surface. */
    private static Map<String, Object> call(CoreCall body, String protocol) {
        try {
            return body.run();
        } catch (LingXiCoreException e) {
            String code = e.code();
            if (code == null) {
                String message = e.getMessage() == null ? "" : e.getMessage();
                int open = message.indexOf('[');
                int close = message.indexOf(']');
                if (open >= 0 && close > open) {
                    code = message.substring(open + 1, close);
                }
            }
            if ("unsupported_feature".equals(code)) {
                String message = e.getMessage() == null ? "" : e.getMessage();
                throw new UnsupportedFeatureException(message, protocol, featureName(message));
            }
            throw e;
        }
    }

    /** Extracts the feature name from "unsupported feature 'x' for protocol 'y'". */
    private static String featureName(String message) {
        int start = message.indexOf("unsupported feature '");
        if (start < 0) {
            return "unknown";
        }
        start += "unsupported feature '".length();
        int end = message.indexOf('\'', start);
        return end < 0 ? "unknown" : message.substring(start, end);
    }
}
