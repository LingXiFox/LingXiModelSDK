#pragma once

#include "lingxi_core.h"
#include <stdexcept>
#include <string>

namespace lingxi::registry {

inline std::string resolve_endpoint(
    const std::string& registry_json,
    const std::string& provider_id,
    const std::string& protocol) {
    char* raw = lingxi_registry_resolve_endpoint(
        registry_json.c_str(), provider_id.c_str(), protocol.c_str());
    if (!raw) {
        throw std::runtime_error("null response from lingxi_registry_resolve_endpoint");
    }
    std::string out(raw);
    lingxi_string_free(raw);
    if (out.find("\"ok\":true") == std::string::npos) {
        throw std::runtime_error("registry resolve failed: " + out);
    }
    return out;
}

} // namespace lingxi::registry
