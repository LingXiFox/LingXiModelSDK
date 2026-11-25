#pragma once

#include "lingxi/core.hpp"
#include "lingxi/json.hpp"
#include "lingxi/native.hpp"

#include <map>
#include <optional>
#include <string>
#include <vector>
namespace lingxi::unified {

// A Unified request prepared for the wire: the LMP request was translated by
// the core, so `target.body` is the provider-native payload for the protocol.
struct Prepared {
    native::Target target;
    json::Value request;
};

inline Prepared prepare(
    const std::string& protocol,
    const std::string& base_url,
    const std::optional<std::string>& api_key,
    const std::optional<std::string>& model,
    const json::Value& lmp_request,
    const native::PrepareOptions& options = {}) {
    json::Value request(json::Object{});
    request.set("protocol", json::Value(protocol));
    request.set("baseUrl", json::Value(base_url));
    if (api_key) request.set("apiKey", json::Value(*api_key));
    if (model) request.set("model", json::Value(*model));
    request.set("request", lmp_request);
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

    json::Value envelope = Core::prepare_unified(request);
    Prepared prepared;
    prepared.request = lmp_request;
    if (const json::Value* url = envelope.find("url"); url && url->is_string()) {
        prepared.target.url = url->string();
    }
    if (const json::Value* headers = envelope.find("headers"); headers && headers->is_object()) {
        for (const auto& entry : headers->object()) {
            if (entry.second.is_string()) prepared.target.headers[entry.first] = entry.second.string();
        }
    }
    if (const json::Value* body = envelope.find("body"); body) prepared.target.body = *body;
    if (const json::Value* framing = envelope.find("framing"); framing && framing->is_string()) {
        prepared.target.framing = framing->string();
    }
    return prepared;
}

// Non-streaming completion: returns the LMP response envelope value (the
// `response` object), never raw provider JSON.
inline json::Value normalize_response(const std::string& protocol, const json::Value& native_response) {
    return Core::normalize_response(protocol, native_response);
}

// Embeddings: returns the LMP EmbeddingResponse envelope value.
inline json::Value embed(
    const std::string& protocol,
    const std::string& base_url,
    const std::optional<std::string>& api_key,
    const std::string& model,
    const std::vector<std::string>& input) {
    json::Value request(json::Object{});
    request.set("protocol", json::Value(protocol));
    request.set("baseUrl", json::Value(base_url));
    if (api_key) request.set("apiKey", json::Value(*api_key));
    request.set("model", json::Value(model));
    json::Value values(json::Array{});
    for (const auto& text : input) values.push(json::Value(text));
    request.set("input", values);
    return Core::embed(request);
}

// Reranking: Cohere only, returns the LMP RerankResponse envelope value.
inline json::Value rerank(
    const std::string& protocol,
    const std::string& base_url,
    const std::optional<std::string>& api_key,
    const std::string& model,
    const std::string& query,
    const std::vector<std::string>& documents,
    const std::optional<long long>& top_n = std::nullopt) {
    json::Value request(json::Object{});
    request.set("protocol", json::Value(protocol));
    request.set("baseUrl", json::Value(base_url));
    if (api_key) request.set("apiKey", json::Value(*api_key));
    request.set("model", json::Value(model));
    request.set("query", json::Value(query));
    json::Value values(json::Array{});
    for (const auto& document : documents) values.push(json::Value(document));
    request.set("documents", values);
    if (top_n) request.set("topN", json::Value(*top_n));
    return Core::rerank(request);
}

} // namespace lingxi::unified
