#include "lingxi/catalog.hpp"
#include "lingxi/native.hpp"
#include "lingxi/registry.hpp"
#include "lingxi/unified.hpp"

#include <cassert>
#include <fstream>
#include <iostream>
#include <sstream>

static std::string read_file(const std::string& path) {
    std::ifstream ifs(path);
    assert(ifs.good());
    std::ostringstream ss;
    ss << ifs.rdbuf();
    return ss.str();
}

int main() {
    // 1. Standalone Catalog (zero FFI)
    lingxi::catalog::ModelCatalog cat(R"({"schemaVersion":"2.0","providers":{"openai":{"id":"openai"}}})");
    assert(cat.contains_provider("openai"));

    // 2. Registry via Rust C ABI
    auto reg_json = read_file("../../registry/registry.json");
    auto resolved = lingxi::registry::resolve_endpoint(reg_json, "amazon-bedrock", "aws_bedrock");
    assert(resolved.find("bedrock-runtime") != std::string::npos);

    // Invalid input returns an error through the same ABI used for network requests.
    auto invalid = lingxi::native::request(R"({"protocol":"unknown","baseUrl":"https://example.invalid","payload":{}})");
    assert(invalid.find("unknown protocol") != std::string::npos);

    // 3. Native endpoint builder (9 protocols)
    assert(lingxi::native::build_endpoint_path("cohere", "command-a") == "/v2/chat");
    assert(lingxi::native::build_endpoint_path("ollama", "llama3.3") == "/api/chat");

    // 4. Unified LMP via Rust C ABI (Tier-2 Bedrock + Vertex + Cohere)
    const std::string lmp_req = R"({
        "model":"anthropic.claude-3-5-sonnet",
        "messages":[{"role":"user","content":[{"type":"text","text":"hello"}]}],
        "thinking":{"budgetTokens":2048}
    })";
    auto bedrock_payload = lingxi::unified::UnifiedBridge::translate_request("aws_bedrock", lmp_req);
    assert(bedrock_payload.find("budget_tokens") != std::string::npos);

    auto bedrock_resp = read_file("../../conformance/native/aws_bedrock_response.json");
    auto norm = lingxi::unified::UnifiedBridge::normalize_response("aws_bedrock", bedrock_resp);
    assert(norm.find("sig-bedrock-1") != std::string::npos);

    // Explicit unsupported error check (Ollama + json_schema strict)
    bool threw = false;
    try {
        lingxi::unified::UnifiedBridge::translate_request(
            "ollama",
            R"({"model":"llama3.3","messages":[{"role":"user","content":[{"type":"text","text":"hi"}]}],"responseFormat":{"type":"json_schema","name":"s","schema":{}}})"
        );
    } catch (const std::runtime_error&) {
        threw = true;
    }
    assert(threw);

    std::cout << "C++ SDK 4-module tests passed!\n";
    return 0;
}
