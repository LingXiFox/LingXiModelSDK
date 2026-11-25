#pragma once

#include "lingxi/core.hpp"
#include "lingxi/json.hpp"

#include <map>
#include <optional>
#include <string>

namespace lingxi::native {

// Everything needed to send one provider-native request. The Rust core built
// the URL, the auth headers (including AWS SigV4) and the body, so no SDK
// re-implements endpoint joining or signing.
struct Target {
    std::string url;
    std::map<std::string, std::string> headers;
    json::Value body;
    std::string framing;  // "sse", "ndjson" or "amazon_event_stream"
};

struct PrepareOptions {
    std::map<std::string, std::string> headers;
    std::optional<long long> timeout_ms;
    std::optional<long long> idle_timeout_ms;
    bool allow_insecure_http = false;
    bool stream = false;
};

inline json::Value build_prepare_request(
    const std::string& protocol,
    const std::string& base_url,
    const std::optional<std::string>& api_key,
    const std::optional<std::string>& model,
    const json::Value& payload,
    const PrepareOptions& options) {
    json::Value request(json::Object{});
    request.set("protocol", json::Value(protocol));
    request.set("baseUrl", json::Value(base_url));
    if (api_key) request.set("apiKey", json::Value(*api_key));
    if (model) request.set("model", json::Value(*model));
    request.set("payload", payload);
    if (!options.headers.empty()) {
        json::Value headers(json::Object{});
        for (const auto& entry : options.headers) {
            headers.set(entry.first, json::Value(entry.second));
        }
        request.set("headers", headers);
    }
    if (options.timeout_ms) request.set("timeoutMs", json::Value(*options.timeout_ms));
    if (options.idle_timeout_ms) request.set("idleTimeoutMs", json::Value(*options.idle_timeout_ms));
    request.set("allowInsecureHttp", json::Value(options.allow_insecure_http));
    request.set("stream", json::Value(options.stream));
    return request;
}

inline Target prepare(
    const std::string& protocol,
    const std::string& base_url,
    const std::optional<std::string>& api_key,
    const std::optional<std::string>& model,
    const json::Value& payload,
    const PrepareOptions& options = {}) {
    json::Value envelope = Core::prepare_native(
        build_prepare_request(protocol, base_url, api_key, model, payload, options));
    Target target;
    if (const json::Value* url = envelope.find("url"); url && url->is_string()) {
        target.url = url->string();
    }
    if (const json::Value* headers = envelope.find("headers"); headers && headers->is_object()) {
        for (const auto& entry : headers->object()) {
            if (entry.second.is_string()) target.headers[entry.first] = entry.second.string();
        }
    }
    if (const json::Value* body = envelope.find("body"); body) target.body = *body;
    if (const json::Value* framing = envelope.find("framing"); framing && framing->is_string()) {
        target.framing = framing->string();
    }
    return target;
}

} // namespace lingxi::native
