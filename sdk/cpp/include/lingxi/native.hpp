#pragma once

#include "lingxi_core.h"
#include <memory>
#include <stdexcept>
#include <string>

namespace lingxi::native {

struct NativeEndpoint {
    std::string protocol;
    std::string base_url;
};

// Request JSON defaults to Native Provider. Set unified=true to send an LMP payload.
inline std::string request(const std::string& request_json) {
    std::unique_ptr<char, decltype(&lingxi_string_free)> response(
        lingxi_runtime_request(request_json.c_str()), &lingxi_string_free);
    if (!response) throw std::runtime_error("Empty response from LingXi runtime");
    return response.get();
}

inline std::string build_endpoint_path(const std::string& protocol, const std::string& model) {
    if (protocol == "openai_chat_completions") return "/chat/completions";
    if (protocol == "openai_responses") return "/responses";
    if (protocol == "anthropic_messages") return "/v1/messages";
    if (protocol == "google_gemini") return "/v1beta/models/" + model + ":generateContent";
    if (protocol == "aws_bedrock") return "/model/" + model + "/converse";
    if (protocol == "google_vertex_ai") return "/v1/publishers/google/models/" + model + ":generateContent";
    if (protocol == "azure_openai") return "/openai/deployments/" + model + "/chat/completions?api-version=2024-10-21";
    if (protocol == "ollama") return "/api/chat";
    if (protocol == "cohere") return "/v2/chat";
    throw std::runtime_error("Unsupported protocol: " + protocol);
}

} // namespace lingxi::native
