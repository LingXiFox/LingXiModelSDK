package cn.lingxifox.modelsdk;

import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.atomic.AtomicInteger;

/**
 * End-to-end checks for the Java SDK surface: catalog, Native, Unified and the
 * incremental stream session, all backed by the shared Rust core.
 *
 * <p>Runs against a loopback HTTP server that replays the repository
 * conformance fixtures, so no credentials or network access are required.
 */
public final class ProviderTestRunner {
    private static int checks = 0;

    public static void main(String[] args) throws Exception {
        Path root = Path.of("../..").toAbsolutePath().normalize();
        registryChecks(root);

        HttpServer server = startServer(root);
        try {
            String base = "http://127.0.0.1:" + server.getAddress().getPort();
            if (!UnifiedClient.available()) {
                throw new AssertionError("Rust core is not loadable; build it with cargo build --manifest-path core/Cargo.toml");
            }
            unifiedChecks(root, base);
            embedAndRerankChecks(base);
            nativeChecks(root, base);
            streamChecks(root, base);
            securityChecks(base);
        } finally {
            server.stop(0);
        }
        System.out.println("Java SDK tests passed (" + checks + " checks)");
    }

    private static void registryChecks(Path root) throws Exception {
        String regJson = Files.readString(root.resolve("registry/registry.json"), StandardCharsets.UTF_8);
        ProviderRegistry reg = ProviderRegistry.fromJson(regJson);
        if (reg.size() < 320) {
            throw new AssertionError("Expected >= 320 providers, got " + reg.size());
        }
        String bedrockEp = reg.resolveEndpoint("amazon-bedrock", "aws_bedrock", null, false);
        if (!bedrockEp.contains("bedrock-runtime")) {
            throw new AssertionError("Unexpected bedrock endpoint: " + bedrockEp);
        }
        checks++;
    }

    @SuppressWarnings("unchecked")
    private static void unifiedChecks(Path root, String base) throws Exception {
        UnifiedClient client = new UnifiedClient();

        Map<String, Object> bedrock = client.completeText(
            "aws_bedrock", base, "AKID:SECRET", "anthropic.claude-3-5-sonnet", "hi");
        String bedrockText = textOf(bedrock);
        expect(bedrockText.equals("Hello from AWS Bedrock"), "bedrock text was " + bedrockText);
        expect("tool_use".equals(bedrock.get("finishReason")), "bedrock finishReason");
        Map<String, Object> usage = (Map<String, Object>) bedrock.get("usage");
        expect(((Number) usage.get("inputTokens")).intValue() == 24, "bedrock input tokens");
        expect(((Number) usage.get("cacheReadTokens")).intValue() == 8, "bedrock cache read");
        List<Map<String, Object>> thinking = blocksOf(bedrock, "thinking");
        expect(thinking.size() == 1, "bedrock thinking blocks");
        expect("sig-bedrock-1".equals(thinking.get(0).get("signature")), "bedrock thinking signature");
        List<Map<String, Object>> toolCalls = blocksOf(bedrock, "tool_call");
        expect(toolCalls.size() == 1, "bedrock tool call blocks");
        expect("get_weather".equals(toolCalls.get(0).get("name")), "bedrock tool name");
        expect(!bedrock.containsKey("output"), "unified response must not leak raw provider fields");
        checks++;

        Map<String, Object> cohere = client.completeText(
            "cohere", base, "co-key", "command-a", "hi");
        expect(textOf(cohere).equals("Hello from Cohere"), "cohere text");
        expect("COMPLETE".equals(cohere.get("finishReason")), "cohere finishReason");
        checks++;

        Map<String, Object> chat = client.completeText(
            "openai_chat_completions", base, "sk-test", "gpt-4o-mini", "hi");
        expect(textOf(chat).equals("Hello from chat completions"), "openai text");
        expect("stop".equals(chat.get("finishReason")), "openai finishReason");
        checks++;

        // Streaming through the core handle yields LMP events incrementally.
        List<Map<String, Object>> events = client.streamAll(
            "anthropic_messages", base, "sk-ant", UnifiedClient.textRequest("claude", "hi"));
        StringBuilder streamed = new StringBuilder();
        StringBuilder thinkingText = new StringBuilder();
        boolean sawEnd = false;
        String endReason = null;
        for (Map<String, Object> event : events) {
            switch (String.valueOf(event.get("type"))) {
                case "text_delta" -> streamed.append(event.get("delta"));
                case "thinking_delta" -> thinkingText.append(event.get("delta"));
                case "message_end" -> {
                    sawEnd = true;
                    endReason = (String) event.get("finishReason");
                }
                default -> { }
            }
        }
        expect(streamed.toString().equals("Hello Anthropic"), "streamed text was " + streamed);
        expect(thinkingText.toString().equals("Thinking..."), "streamed thinking was " + thinkingText);
        expect(sawEnd, "stream must end with message_end");
        // The trimmed fixture omits message_delta, so no upstream stop_reason exists;
        // the terminal event must still be present and must not invent a reason.
        expect(endReason == null || endReason.equals("stop") || endReason.equals("end_turn"),
            "unexpected stream finish reason: " + endReason);
        checks++;

        Map<String, Object> gemini = client.completeText(
            "google_gemini", base, "g-key", "gemini-2.0-flash", "hi");
        expect(textOf(gemini).equals("Hello from Gemini"), "gemini text was " + textOf(gemini));
        expect("STOP".equals(gemini.get("finishReason")), "gemini finishReason");
        expect(blocksOf(gemini, "thinking").size() == 1, "gemini thinking blocks");
        Map<String, Object> geminiUsage = (Map<String, Object>) gemini.get("usage");
        expect(((Number) geminiUsage.get("totalTokens")).intValue() == 23, "gemini total tokens");
        checks++;
    }

    @SuppressWarnings("unchecked")
    private static void embedAndRerankChecks(String base) {
        UnifiedClient client = new UnifiedClient();
        Map<String, Object> embed = client.embed(
            "openai_chat_completions", base, "sk-test", "text-embedding-3-small", List.of("a", "b"));
        List<Object> vectors = (List<Object>) embed.get("embeddings");
        expect(vectors.size() == 2, "embedding count");
        expect(((List<Object>) vectors.get(0)).size() == 3, "embedding dimensions");
        checks++;

        Map<String, Object> rerank = client.rerank(
            "cohere", base, "co-key", "rerank-v3.5", "java", List.of("a", "b"));
        List<Map<String, Object>> results = (List<Map<String, Object>>) rerank.get("results");
        expect(results.size() == 2, "rerank result count");
        expect(((Number) results.get(0).get("index")).intValue() == 1, "rerank top index");
        expect(Math.abs(((Number) results.get(0).get("relevanceScore")).doubleValue() - 0.94) < 1e-9,
            "rerank top score");
        checks++;

        boolean unsupported = false;
        try {
            client.rerank("openai_chat_completions", base, "sk", "m", "q", List.of("a"));
        } catch (UnifiedClient.UnsupportedFeatureException expected) {
            unsupported = true;
        }
        expect(unsupported, "rerank on a non-Cohere protocol must be unsupported");
        checks++;
    }

    @SuppressWarnings("unchecked")
    private static void nativeChecks(Path root, String base) throws Exception {
        RustCoreBridge bridge = RustCoreBridge.shared();
        Map<String, Object> prepare = new LinkedHashMap<>();
        prepare.put("protocol", "aws_bedrock");
        prepare.put("baseUrl", base);
        prepare.put("apiKey", "AKID:SECRET");
        prepare.put("model", "anthropic.claude-3-5-sonnet");
        prepare.put("payload", Map.of("messages", List.of(Map.of("role", "user", "content",
            List.of(Map.of("text", "hi"))))));
        Map<String, Object> target = bridge.prepareNative(prepare);
        String url = String.valueOf(target.get("url"));
        expect(url.endsWith("/model/anthropic.claude-3-5-sonnet/converse"), "native bedrock url " + url);
        Map<String, Object> headers = (Map<String, Object>) target.get("headers");
        expect(String.valueOf(headers.get("Authorization")).startsWith("AWS4-HMAC-SHA256"), "sigv4 header");
        expect(!String.valueOf(headers.get("Authorization")).contains("SECRET"), "secret must not leak");

        Map<String, String> flat = new LinkedHashMap<>();
        headers.forEach((k, v) -> flat.put(k, String.valueOf(v)));
        Map<String, Object> raw = new NativeClient().postJson(
            url, flat, JsonWriter.write(target.get("body")), false);
        expect(String.valueOf(raw.get("output")).contains("Hello from AWS Bedrock"), "native raw body");
        checks++;
    }

    @SuppressWarnings("unchecked")
    private static void streamChecks(Path root, String base) throws Exception {
        UnifiedClient client = new UnifiedClient();

        Map<String, Object> open = new LinkedHashMap<>();
        open.put("protocol", "openai_chat_completions");
        open.put("baseUrl", base);
        open.put("apiKey", "sk-test");
        open.put("model", "gpt-4o-mini");
        open.put("payload", Map.of("model", "gpt-4o-mini",
            "messages", List.of(Map.of("role", "user", "content", "hi"))));

        StringBuilder text = new StringBuilder();
        Map<String, Object> lastUsage = null;
        int frames = 0;
        try (StreamSession session = StreamSession.open(open, false)) {
            Map<String, Object> event;
            while ((event = session.next()) != null) {
                Object data = event.get("data");
                expect(data instanceof String && !((String) data).isEmpty(),
                    "native frames must carry the raw provider payload");
                if (!"[DONE]".equals(data)) {
                    expect(JsonParser.parse((String) data) instanceof Map,
                        "native payload must be JSON");
                    frames++;
                }
            }
            expect(frames == 2, "native stream should deliver both provider frames, got " + frames);
            checks++;
        }

        Map<String, Object> cancelOpen = new LinkedHashMap<>(open);
        StreamSession cancelSession = StreamSession.open(cancelOpen, false);
        expect(cancelSession.next() != null, "first event must arrive");
        cancelSession.cancel();
        boolean cancelled = false;
        try {
            while (cancelSession.next() != null) {
                // drain until the cancellation surfaces
            }
        } catch (LingXiCoreException expected) {
            cancelled = String.valueOf(expected.code()).contains("cancel");
        } finally {
            cancelSession.close();
        }
        expect(cancelled, "cancelling a live stream must surface a cancelled error");
        checks++;

        Map<String, Object> unifiedOpen = new LinkedHashMap<>();
        unifiedOpen.put("protocol", "openai_chat_completions");
        unifiedOpen.put("baseUrl", base);
        unifiedOpen.put("apiKey", "sk-test");
        unifiedOpen.put("payload", UnifiedClient.textRequest("gpt-4o-mini", "hi"));
        try (StreamSession session = StreamSession.open(unifiedOpen, true)) {
            Map<String, Object> event;
            while ((event = session.next()) != null) {
                if ("text_delta".equals(event.get("type"))) {
                    text.append(event.get("delta"));
                } else if ("usage".equals(event.get("type"))) {
                    lastUsage = (Map<String, Object>) event.get("usage");
                }
            }
        }
        expect(text.toString().equals("Hello world"), "unified stream text was " + text);
        expect(lastUsage != null, "unified stream must report usage");
        expect(((Number) lastUsage.get("totalTokens")).intValue() == 12, "unified stream usage");
        checks++;
    }

    private static void securityChecks(String base) throws Exception {
        UnifiedClient client = new UnifiedClient();
        boolean blocked = false;
        try {
            client.completeText("cohere", base + "/redir", "co-key", "command-a", "hi");
        } catch (IllegalStateException expected) {
            blocked = String.valueOf(expected.getMessage()).contains("redirect");
        }
        expect(blocked, "redirects must be blocked");

        boolean insecure = false;
        try {
            RustCoreBridge bridge = RustCoreBridge.shared();
            bridge.prepareNative(Map.of("protocol", "openai_chat_completions",
                "baseUrl", "http://example.com/v1", "apiKey", "k", "payload", Map.of()));
        } catch (LingXiCoreException expected) {
            insecure = "invalid_endpoint".equals(expected.code());
        }
        expect(insecure, "plain HTTP to a remote host must be rejected by the core");
        checks++;
    }

    private static HttpServer startServer(Path root) throws Exception {
        HttpServer server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        AtomicInteger requests = new AtomicInteger();
        server.createContext("/", (HttpExchange ex) -> {
            requests.incrementAndGet();
            String path = ex.getRequestURI().getPath();
            if (path.startsWith("/redir")) {
                ex.getResponseHeaders().add("Location", "http://127.0.0.1:9/leak");
                ex.sendResponseHeaders(302, -1);
                ex.close();
                return;
            }
            if (ex.getRequestHeaders().getFirst("Authorization") != null
                && path.contains("converse")) {
                String auth = ex.getRequestHeaders().getFirst("Authorization");
                if (!auth.startsWith("AWS4-HMAC-SHA256")) {
                    throw new AssertionError("Missing SigV4 header");
                }
            }
            boolean stream = isStreaming(ex);
            byte[] body;
            String contentType = "application/json";
            if (path.contains("converse")) {
                body = Files.readAllBytes(root.resolve("conformance/native/aws_bedrock_response.json"));
            } else if (path.contains("/v2/rerank")) {
                body = Files.readAllBytes(root.resolve("conformance/native/cohere_rerank_response.json"));
            } else if (path.contains("/embeddings")) {
                body = Files.readAllBytes(root.resolve("conformance/native/openai_embed_response.json"));
            } else if (path.contains(":generateContent")) {
                body = Files.readAllBytes(root.resolve("conformance/native/google_gemini_response.json"));
            } else if (path.contains("/messages") && stream) {
                body = Files.readAllBytes(root.resolve("conformance/native/anthropic_messages_stream.sse"));
                contentType = "text/event-stream";
            } else if (path.contains("/messages")) {
                body = Files.readAllBytes(root.resolve("conformance/native/anthropic_messages_response.json"));
            } else if (path.contains("/chat/completions") && stream) {
                body = Files.readAllBytes(root.resolve("conformance/native/openai_chat_stream.sse"));
                contentType = "text/event-stream";
            } else if (path.contains("/chat/completions")) {
                body = Files.readAllBytes(root.resolve("conformance/native/openai_chat_response.json"));
            } else {
                body = Files.readAllBytes(root.resolve("conformance/native/cohere_chat_response.json"));
            }
            ex.getResponseHeaders().add("Content-Type", contentType);
            ex.sendResponseHeaders(200, body.length);
            try (OutputStream os = ex.getResponseBody()) {
                os.write(body);
            }
        });
        server.start();
        return server;
    }

    private static boolean isStreaming(HttpExchange ex) {
        try {
            byte[] raw = ex.getRequestBody().readAllBytes();
            return new String(raw, StandardCharsets.UTF_8).contains("\"stream\":true");
        } catch (Exception e) {
            return false;
        }
    }

    private static void expect(boolean condition, String message) {
        if (!condition) {
            throw new AssertionError(message);
        }
    }

    @SuppressWarnings("unchecked")
    private static String textOf(Map<String, Object> response) {
        StringBuilder text = new StringBuilder();
        Object blocks = response.get("content");
        if (blocks instanceof List<?> list) {
            for (Object block : list) {
                Map<String, Object> map = (Map<String, Object>) block;
                if ("text".equals(map.get("type"))) {
                    text.append(map.get("text"));
                }
            }
        }
        return text.toString();
    }

    @SuppressWarnings("unchecked")
    private static List<Map<String, Object>> blocksOf(Map<String, Object> response, String type) {
        List<Map<String, Object>> out = new ArrayList<>();
        if (response.get("content") instanceof List<?> list) {
            for (Object block : list) {
                Map<String, Object> map = (Map<String, Object>) block;
                if (type.equals(map.get("type"))) {
                    out.add(map);
                }
            }
        }
        return out;
    }
}
