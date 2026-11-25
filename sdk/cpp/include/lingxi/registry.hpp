#pragma once

#include "lingxi/core.hpp"
#include "lingxi/json.hpp"

#include <optional>
#include <string>

namespace lingxi::registry {

struct Endpoint {
    std::string endpoint;
    std::optional<std::string> provider;
    std::optional<std::string> auth_type;
};

// Looks up the provider base URL for a protocol from registry JSON, with the
// validation rules (https-only, known provider) applied by the core.
inline Endpoint resolve_endpoint(
    const std::string& registry_json,
    const std::string& provider_id,
    const std::string& protocol) {
    json::Value envelope = Core::resolve_endpoint(registry_json, provider_id, protocol);
    Endpoint resolved;
    if (const json::Value* endpoint = envelope.find("endpoint"); endpoint && endpoint->is_string()) {
        resolved.endpoint = endpoint->string();
    }
    if (const json::Value* provider = envelope.find("provider"); provider && provider->is_string()) {
        resolved.provider = provider->string();
    }
    if (const json::Value* auth = envelope.find("authType"); auth && auth->is_string()) {
        resolved.auth_type = auth->string();
    }
    return resolved;
}

} // namespace lingxi::registry
