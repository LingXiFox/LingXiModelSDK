package cn.lingxifox.modelsdk;

import com.sun.net.httpserver.HttpServer;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;

public final class ProviderTestRunner {
    public static void main(String[] args) throws Exception {
        Path root = Path.of("../..").toAbsolutePath().normalize();
        String regJson = Files.readString(root.resolve("registry/registry.json"), StandardCharsets.UTF_8);
        ProviderRegistry reg = ProviderRegistry.fromJson(regJson);
        if (reg.size() < 320) {
            throw new AssertionError("Expected >= 320 providers, got " + reg.size());
        }
        String bedrockEp = reg.resolveEndpoint("amazon-bedrock", "aws_bedrock", null, false);
        if (!bedrockEp.contains("bedrock-runtime")) {
            throw new AssertionError("Unexpected bedrock endpoint: " + bedrockEp);
        }

        HttpServer server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/", ex -> {
            String path = ex.getRequestURI().getPath();
            if (path.startsWith("/redir")) {
                ex.getResponseHeaders().add("Location", "http://127.0.0.1:9/leak");
                ex.sendResponseHeaders(302, -1);
                ex.close();
                return;
            }
            byte[] body;
            if (path.contains("converse")) {
                String auth = ex.getRequestHeaders().getFirst("Authorization");
                if (auth == null || !auth.startsWith("AWS4-HMAC-SHA256")) {
                    throw new AssertionError("Missing SigV4 header");
                }
                body = Files.readAllBytes(root.resolve("conformance/native/aws_bedrock_response.json"));
            } else if (path.contains("/v2/rerank")) {
                body = Files.readAllBytes(root.resolve("conformance/native/cohere_rerank_response.json"));
            } else {
                body = Files.readAllBytes(root.resolve("conformance/native/cohere_chat_response.json"));
            }
            ex.getResponseHeaders().add("Content-Type", "application/json");
            ex.sendResponseHeaders(200, body.length);
            try (OutputStream os = ex.getResponseBody()) {
                os.write(body);
            }
        });
        server.start();
        try {
            String base = "http://127.0.0.1:" + server.getAddress().getPort();
            UnifiedClient client = new UnifiedClient();
            Map<String, Object> bRes = client.completeText("aws_bedrock", base, "AKIA:SECRET", "anthropic.claude-3-5-sonnet", "hi");
            if (!bRes.containsKey("output")) throw new AssertionError("Missing bedrock output");

            Map<String, Object> rr = client.rerank("cohere", base, "co-key", "rerank-v3.5", "java", List.of("a", "b"));
            if (!rr.containsKey("results")) throw new AssertionError("Missing rerank results");

            boolean threwUnsupported = false;
            try {
                client.rerank("openai_chat_completions", base, "sk", "m", "q", List.of("a"));
            } catch (UnifiedClient.UnsupportedFeatureException expected) {
                threwUnsupported = true;
            }
            if (!threwUnsupported) throw new AssertionError("Expected UnsupportedFeatureException");

            boolean threwRedirect = false;
            try {
                client.completeText("cohere", base + "/redir", "co-key", "command-a", "hi");
            } catch (IllegalStateException expected) {
                threwRedirect = true;
            }
            if (!threwRedirect) throw new AssertionError("Expected redirect block");

            System.out.println("Java 4-module SDK tests passed!");
        } finally {
            server.stop(0);
        }
    }
}
