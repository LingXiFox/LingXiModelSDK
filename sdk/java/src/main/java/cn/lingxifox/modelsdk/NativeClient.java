package cn.lingxifox.modelsdk;

import javax.crypto.Mac;
import javax.crypto.spec.SecretKeySpec;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.time.Duration;
import java.time.ZoneOffset;
import java.time.ZonedDateTime;
import java.time.format.DateTimeFormatter;
import java.util.LinkedHashMap;
import java.util.Map;

public final class NativeClient {
    private final HttpClient http;

    public NativeClient() {
        this.http = HttpClient.newBuilder()
            .followRedirects(HttpClient.Redirect.NEVER)
            .connectTimeout(Duration.ofSeconds(15))
            .build();
    }

    public Map<String, Object> postJson(
        String url,
        Map<String, String> headers,
        String jsonBody,
        boolean allowInsecureHttp
    ) throws Exception {
        ProviderRegistry.validateEndpointUrl(url, allowInsecureHttp);
        HttpRequest.Builder b = HttpRequest.newBuilder(URI.create(url))
            .timeout(Duration.ofSeconds(60))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .POST(HttpRequest.BodyPublishers.ofString(jsonBody, StandardCharsets.UTF_8));
        for (Map.Entry<String, String> e : headers.entrySet()) {
            b.header(e.getKey(), e.getValue());
        }
        HttpResponse<String> resp = http.send(b.build(), HttpResponse.BodyHandlers.ofString(StandardCharsets.UTF_8));
        if (resp.statusCode() >= 300 && resp.statusCode() <= 399) {
            throw new IllegalStateException("HTTP redirect " + resp.statusCode() + " blocked for security");
        }
        if (resp.statusCode() < 200 || resp.statusCode() > 299) {
            throw new IllegalStateException("Upstream HTTP " + resp.statusCode() + ": " + resp.body());
        }
        @SuppressWarnings("unchecked")
        Map<String, Object> map = (Map<String, Object>) JsonParser.parse(resp.body());
        return map;
    }

    public Map<String, String> buildAuthHeaders(
        String protocol,
        String baseUrl,
        String apiKey,
        String canonicalUri,
        String payloadJson
    ) throws Exception {
        Map<String, String> hdrs = new LinkedHashMap<>();
        switch (protocol) {
            case "openai_chat_completions", "openai_responses", "google_vertex_ai", "cohere" -> {
                if (apiKey == null || apiKey.isEmpty()) throw new IllegalArgumentException("Bearer API key required");
                hdrs.put("Authorization", "Bearer " + apiKey);
            }
            case "anthropic_messages" -> {
                if (apiKey == null || apiKey.isEmpty()) throw new IllegalArgumentException("x-api-key required");
                hdrs.put("x-api-key", apiKey);
                hdrs.put("anthropic-version", "2023-06-01");
            }
            case "google_gemini" -> {
                if (apiKey == null || apiKey.isEmpty()) throw new IllegalArgumentException("x-goog-api-key required");
                hdrs.put("x-goog-api-key", apiKey);
            }
            case "azure_openai" -> {
                if (apiKey == null || apiKey.isEmpty()) throw new IllegalArgumentException("api-key required");
                hdrs.put("api-key", apiKey);
            }
            case "ollama" -> { /* local no-auth default */ }
            case "aws_bedrock" -> {
                if (apiKey == null || apiKey.isEmpty()) throw new IllegalArgumentException("Bedrock credentials required");
                String[] parts = apiKey.split(":");
                if (parts.length >= 2) {
                    URI u = URI.create(baseUrl);
                    String host = u.getPort() > 0 ? u.getHost() + ":" + u.getPort() : u.getHost();
                    hdrs.putAll(signSigV4("POST", canonicalUri, host, "us-east-1", "bedrock", parts[0], parts[1],
                        parts.length >= 3 ? parts[2] : null, payloadJson.getBytes(StandardCharsets.UTF_8)));
                } else {
                    hdrs.put("Authorization", "Bearer " + apiKey);
                }
            }
            default -> throw new IllegalArgumentException("Unsupported protocol: " + protocol);
        }
        return hdrs;
    }

    public static Map<String, String> signSigV4(
        String method,
        String canonicalUri,
        String host,
        String region,
        String service,
        String accessKey,
        String secretKey,
        String sessionToken,
        byte[] payload
    ) throws Exception {
        String amzDate = DateTimeFormatter.ofPattern("yyyyMMdd'T'HHmmss'Z'")
            .format(ZonedDateTime.now(ZoneOffset.UTC));
        String dateStamp = amzDate.substring(0, 8);
        String payloadHash = sha256Hex(payload);

        StringBuilder canonicalHeaders = new StringBuilder()
            .append("host:").append(host).append('\n')
            .append("x-amz-date:").append(amzDate).append('\n');
        String signedHeaders = "host;x-amz-date";
        if (sessionToken != null && !sessionToken.isEmpty()) {
            canonicalHeaders.append("x-amz-security-token:").append(sessionToken).append('\n');
            signedHeaders = "host;x-amz-date;x-amz-security-token";
        }
        String canonicalReq = method + "\n" + canonicalUri + "\n\n" + canonicalHeaders + "\n" + signedHeaders + "\n" + payloadHash;
        String scope = dateStamp + "/" + region + "/" + service + "/aws4_request";
        String sts = "AWS4-HMAC-SHA256\n" + amzDate + "\n" + scope + "\n" + sha256Hex(canonicalReq.getBytes(StandardCharsets.UTF_8));

        byte[] kDate = hmacSha256(("AWS4" + secretKey).getBytes(StandardCharsets.UTF_8), dateStamp);
        byte[] kRegion = hmacSha256(kDate, region);
        byte[] kService = hmacSha256(kRegion, service);
        byte[] kSigning = hmacSha256(kService, "aws4_request");
        String sig = toHex(hmacSha256(kSigning, sts));

        Map<String, String> out = new LinkedHashMap<>();
        out.put("Authorization", "AWS4-HMAC-SHA256 Credential=" + accessKey + "/" + scope + ", SignedHeaders=" + signedHeaders + ", Signature=" + sig);
        out.put("X-Amz-Date", amzDate);
        if (sessionToken != null && !sessionToken.isEmpty()) {
            out.put("X-Amz-Security-Token", sessionToken);
        }
        return out;
    }

    private static String sha256Hex(byte[] data) throws Exception {
        return toHex(MessageDigest.getInstance("SHA-256").digest(data));
    }

    private static byte[] hmacSha256(byte[] key, String data) throws Exception {
        Mac mac = Mac.getInstance("HmacSHA256");
        mac.init(new SecretKeySpec(key, "HmacSHA256"));
        return mac.doFinal(data.getBytes(StandardCharsets.UTF_8));
    }

    private static String toHex(byte[] bytes) {
        StringBuilder sb = new StringBuilder(bytes.length * 2);
        for (byte b : bytes) {
            sb.append(String.format("%02x", b));
        }
        return sb.toString();
    }
}
