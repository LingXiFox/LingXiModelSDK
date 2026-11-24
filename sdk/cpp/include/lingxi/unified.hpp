#pragma once

#include "lingxi_core.h"
#include <stdexcept>
#include <string>

namespace lingxi::unified {

class UnifiedBridge {
public:
    static std::string translate_request(
        const std::string& protocol,
        const std::string& lmp_request_json) {
        char* raw = lingxi_lmp_translate_request(protocol.c_str(), lmp_request_json.c_str());
        if (!raw) throw std::runtime_error("null from lingxi_lmp_translate_request");
        std::string out(raw);
        lingxi_string_free(raw);
        if (out.find("\"ok\":true") == std::string::npos) {
            throw std::runtime_error("LMP translate_request failed: " + out);
        }
        return out;
    }

    static std::string normalize_response(
        const std::string& protocol,
        const std::string& native_response_json) {
        char* raw = lingxi_lmp_normalize_response(protocol.c_str(), native_response_json.c_str());
        if (!raw) throw std::runtime_error("null from lingxi_lmp_normalize_response");
        std::string out(raw);
        lingxi_string_free(raw);
        if (out.find("\"ok\":true") == std::string::npos) {
            throw std::runtime_error("LMP normalize_response failed: " + out);
        }
        return out;
    }

    static std::string normalize_stream_events(
        const std::string& protocol,
        const std::string& events_json) {
        char* raw = lingxi_lmp_normalize_stream_events(protocol.c_str(), events_json.c_str());
        if (!raw) throw std::runtime_error("null from lingxi_lmp_normalize_stream_events");
        std::string out(raw);
        lingxi_string_free(raw);
        if (out.find("\"ok\":true") == std::string::npos) {
            throw std::runtime_error("LMP normalize_stream_events failed: " + out);
        }
        return out;
    }
};

} // namespace lingxi::unified
