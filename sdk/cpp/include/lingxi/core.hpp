#pragma once

#include "lingxi/json.hpp"
#include "lingxi_core.h"

#include <stdexcept>
#include <string>
#include <utility>

namespace lingxi {

// Error carrying the machine-readable code from the Rust core envelope
// (`invalid_endpoint`, `auth_missing`, `unsupported_feature`, `decode`, ...),
// so callers can branch on the reason instead of parsing prose.
class CoreError : public std::runtime_error {
public:
    CoreError(std::string operation, std::string code, std::string message)
        : std::runtime_error(operation + " failed [" + code + "]: " + message),
          operation_(std::move(operation)),
          code_(std::move(code)),
          message_(std::move(message)) {}

    const std::string& operation() const { return operation_; }
    const std::string& code() const { return code_; }
    const std::string& message() const { return message_; }

private:
    std::string operation_;
    std::string code_;
    std::string message_;
};

namespace detail {

struct Envelope {
    bool ok = false;
    json::Value value;
    std::string code;
    std::string error;
};

// Parses a core envelope and frees the C string, whether or not it parsed.
inline Envelope parse_envelope(const std::string& operation, char* raw) {
    std::string text;
    if (raw != nullptr) {
        text.assign(raw);
        lingxi_string_free(raw);
    }
    if (text.empty()) {
        throw CoreError(operation, "null_response", "the core returned no payload");
    }
    Envelope envelope;
    try {
        envelope.value = json::Value::parse(text);
    } catch (const std::exception&) {
        throw CoreError(operation, "invalid_envelope", text);
    }
    const json::Value* ok = envelope.value.find("ok");
    envelope.ok = ok && ok->is_bool() && ok->boolean();
    if (const json::Value* code = envelope.value.find("code"); code && code->is_string()) {
        envelope.code = code->string();
    }
    if (const json::Value* error = envelope.value.find("error"); error && error->is_string()) {
        envelope.error = error->string();
    }
    return envelope;
}

inline json::Value require_ok(const std::string& operation, char* raw) {
    Envelope envelope = parse_envelope(operation, raw);
    if (!envelope.ok) {
        throw CoreError(operation, envelope.code.empty() ? "unknown" : envelope.code, envelope.error);
    }
    return envelope.value;
}

// Same, but lifts the named member out of the envelope first: the core wraps
// translated payloads and normalized results as {ok:true,<field>:...}.
inline json::Value require_field(const std::string& operation, char* raw, const char* field) {
    Envelope envelope = parse_envelope(operation, raw);
    if (!envelope.ok) {
        throw CoreError(operation, envelope.code.empty() ? "unknown" : envelope.code, envelope.error);
    }
    const json::Value* value = envelope.value.find(field);
    if (value == nullptr) {
        throw CoreError(operation, "invalid_envelope", std::string("the core envelope has no '") + field + "' member");
    }
    return *value;
}

} // namespace detail

// Thin, typed facade over the C ABI shared by every language binding.
struct Core {
    // LMP request -> provider-native payload.
    static json::Value translate_request(const std::string& protocol, const json::Value& request) {
        std::string body = request.dump();
        return detail::require_field(
            "lingxi_lmp_translate_request",
            lingxi_lmp_translate_request(protocol.c_str(), body.c_str()),
            "payload");
    }

    // Provider-native response -> LMP response.
    static json::Value normalize_response(const std::string& protocol, const json::Value& native) {
        std::string body = native.dump();
        return detail::require_field(
            "lingxi_lmp_normalize_response",
            lingxi_lmp_normalize_response(protocol.c_str(), body.c_str()),
            "response");
    }

    // Complete event array -> LMP stream events (buffered; prefer NormalizerSession).
    static json::Value normalize_stream_events(const std::string& protocol, const json::Value& events) {
        std::string body = events.dump();
        return detail::require_field(
            "lingxi_lmp_normalize_stream_events",
            lingxi_lmp_normalize_stream_events(protocol.c_str(), body.c_str()),
            "events");
    }

    // URL + auth headers + body + framing for a provider-native payload.
    static json::Value prepare_native(const json::Value& request) {
        std::string body = request.dump();
        return detail::require_ok("lingxi_native_prepare", lingxi_native_prepare(body.c_str()));
    }

    // Same, starting from an LMP request (the Unified path).
    static json::Value prepare_unified(const json::Value& request) {
        std::string body = request.dump();
        return detail::require_ok("lingxi_unified_prepare", lingxi_unified_prepare(body.c_str()));
    }

    static json::Value embed(const json::Value& request) {
        std::string body = request.dump();
        return detail::require_field("lingxi_unified_embed", lingxi_unified_embed(body.c_str()), "response");
    }

    static json::Value rerank(const json::Value& request) {
        std::string body = request.dump();
        return detail::require_field("lingxi_unified_rerank", lingxi_unified_rerank(body.c_str()), "response");
    }

    static json::Value resolve_endpoint(
        const std::string& registry_json,
        const std::string& provider_id,
        const std::string& protocol) {
        return detail::require_ok(
            "lingxi_registry_resolve_endpoint",
            lingxi_registry_resolve_endpoint(registry_json.c_str(), provider_id.c_str(), protocol.c_str()));
    }
};

} // namespace lingxi
