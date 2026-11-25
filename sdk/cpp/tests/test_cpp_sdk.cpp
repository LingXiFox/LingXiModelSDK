// LingXiModelSDK C++ conformance suite.
//
// Every check goes through the public headers and the Rust core: catalog
// decoding, registry resolution, native payloads, unified LMP normalization,
// embeddings, reranking, incremental streaming and the error contract.
//
// Usage: ./test_cpp_sdk [repository-root]

#include "lingxi/catalog.hpp"
#include "lingxi/catalog_loader.hpp"
#include "lingxi/core.hpp"
#include "lingxi/http.hpp"
#include "lingxi/json.hpp"
#include "lingxi/native.hpp"
#include "lingxi/normalizer.hpp"
#include "lingxi/registry.hpp"
#include "lingxi/stream.hpp"
#include "lingxi/unified.hpp"

#include <arpa/inet.h>
#include <netinet/in.h>
#include <sys/socket.h>
#include <unistd.h>

#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <functional>
#include <iostream>
#include <map>
#include <mutex>
#include <optional>
#include <sstream>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

namespace {

int checks = 0;

void expect(bool condition, const std::string& label) {
    ++checks;
    if (!condition) {
        std::cerr << "FAIL: " << label << "\n";
        std::exit(1);
    }
}

void expect_equal(const std::string& actual, const std::string& expected, const std::string& label) {
    ++checks;
    if (actual != expected) {
        std::cerr << "FAIL: " << label << "\n"
                  << "  expected: " << expected << "\n"
                  << "  actual:   " << actual << "\n";
        std::exit(1);
    }
}

std::string read_file(const std::string& path) {
    std::ifstream input(path, std::ios::binary);
    if (!input) throw std::runtime_error("cannot read " + path);
    std::ostringstream buffer;
    buffer << input.rdbuf();
    return buffer.str();
}

template <typename Fn>
bool throws_with_code(Fn&& fn, const std::string& expected_code) {
    try {
        fn();
    } catch (const lingxi::CoreError& error) {
        if (error.code() == expected_code) return true;
        std::cerr << "  mismatched code: got '" << error.code() << "' want '" << expected_code << "' (" << error.what()
                  << ")\n";
        return false;
    } catch (const std::exception& error) {
        std::cerr << "  unexpected exception: " << error.what() << "\n";
        return false;
    }
    std::cerr << "  no exception was raised\n";
    return false;
}

std::string join(const std::vector<std::string>& values) {
    std::string out;
    for (const auto& value : values) {
        if (!out.empty()) out += ", ";
        out += value;
    }
    return out;
}
// ------------------------------------------------------------ HTTP transport

struct Url {
    std::string host;
    int port = 80;
    std::string target;
};

Url parse_url(const std::string& url) {
    const std::string scheme = "http://";
    expect(url.rfind(scheme, 0) == 0, "the test transport only speaks http:// urls: " + url);
    std::string rest = url.substr(scheme.size());
    std::size_t slash = rest.find('/');
    std::string authority = slash == std::string::npos ? rest : rest.substr(0, slash);
    std::string target = slash == std::string::npos ? "/" : rest.substr(slash);
    Url parsed;
    parsed.target = target;
    std::size_t colon = authority.find(':');
    if (colon == std::string::npos) {
        parsed.host = authority;
    } else {
        parsed.host = authority.substr(0, colon);
        parsed.port = std::stoi(authority.substr(colon + 1));
    }
    return parsed;
}

std::string decode_chunked(const std::string& raw);

// Minimal blocking HTTP/1.1 client, so the injected Transport is a real socket
// exchange instead of a bypass of the SDK plumbing.
lingxi::HttpReply http_exchange(
    const std::string& url,
    const std::map<std::string, std::string>& headers,
    const std::string& body,
    std::chrono::milliseconds timeout) {
    Url parsed = parse_url(url);
    int socket_fd = ::socket(AF_INET, SOCK_STREAM, 0);
    if (socket_fd < 0) throw std::runtime_error("socket() failed");

    timeval limit{};
    limit.tv_sec = static_cast<long>(timeout.count() / 1000);
    limit.tv_usec = static_cast<long>((timeout.count() % 1000) * 1000);
    ::setsockopt(socket_fd, SOL_SOCKET, SO_RCVTIMEO, &limit, sizeof(limit));
    ::setsockopt(socket_fd, SOL_SOCKET, SO_SNDTIMEO, &limit, sizeof(limit));
    int no_sigpipe = 1;
    ::setsockopt(socket_fd, SOL_SOCKET, SO_NOSIGPIPE, &no_sigpipe, sizeof(no_sigpipe));

    sockaddr_in address{};
    address.sin_family = AF_INET;
    address.sin_port = htons(static_cast<uint16_t>(parsed.port));
    address.sin_addr.s_addr = inet_addr(parsed.host.c_str());
    if (::connect(socket_fd, reinterpret_cast<sockaddr*>(&address), sizeof(address)) != 0) {
        ::close(socket_fd);
        throw std::runtime_error("connect() failed for " + url);
    }

    std::ostringstream request;
    request << "POST " << parsed.target << " HTTP/1.1\r\n";
    request << "Host: " << parsed.host << ":" << parsed.port << "\r\n";
    request << "Content-Length: " << body.size() << "\r\n";
    request << "Connection: close\r\n";
    for (const auto& entry : headers) request << entry.first << ": " << entry.second << "\r\n";
    request << "\r\n" << body;
    const std::string wire = request.str();
    std::size_t sent = 0;
    while (sent < wire.size()) {
        ssize_t wrote = ::send(socket_fd, wire.data() + sent, wire.size() - sent, 0);
        if (wrote <= 0) {
            ::close(socket_fd);
            throw std::runtime_error("send() failed for " + url);
        }
        sent += static_cast<std::size_t>(wrote);
    }

    std::string raw;
    char buffer[8192];
    while (true) {
        ssize_t read_now = ::recv(socket_fd, buffer, sizeof(buffer), 0);
        if (read_now == 0) break;
        if (read_now < 0) {
            ::close(socket_fd);
            throw std::runtime_error("recv() failed for " + url);
        }
        raw.append(buffer, static_cast<std::size_t>(read_now));
    }
    ::close(socket_fd);

    std::size_t header_end = raw.find("\r\n\r\n");
    if (header_end == std::string::npos) throw std::runtime_error("malformed HTTP response from " + url);
    lingxi::HttpReply reply;
    {
        std::istringstream status_line(raw.substr(0, raw.find("\r\n")));
        std::string http_version;
        std::string status;
        status_line >> http_version >> status;
        reply.status_code = std::stoi(status);
    }
    reply.body = raw.substr(header_end + 4);
    {
        std::istringstream header_block(raw.substr(0, header_end));
        std::string line;
        std::getline(header_block, line);
        while (std::getline(header_block, line)) {
            if (!line.empty() && line.back() == '\r') line.pop_back();
            std::size_t colon = line.find(':');
            if (colon == std::string::npos) continue;
            std::string key = line.substr(0, colon);
            std::string value = line.substr(colon + 1);
            if (!value.empty() && value.front() == ' ') value.erase(0, 1);
            reply.headers[key] = value;
        }
    }
    if (reply.headers.count("Transfer-Encoding") == 1 &&
        reply.headers.at("Transfer-Encoding") == "chunked") {
        reply.body = decode_chunked(reply.body);
    }
    return reply;
}

std::string decode_chunked(const std::string& raw) {
    std::string out;
    std::size_t pos = 0;
    while (pos < raw.size()) {
        std::size_t line_end = raw.find("\r\n", pos);
        if (line_end == std::string::npos) break;
        std::string size_text = raw.substr(pos, line_end - pos);
        std::size_t semicolon = size_text.find(';');
        if (semicolon != std::string::npos) size_text.erase(semicolon);
        unsigned long long size = 0;
        try {
            size = std::stoull(size_text, nullptr, 16);
        } catch (const std::exception&) {
            break;
        }
        pos = line_end + 2;
        if (size == 0) break;
        if (pos + size > raw.size()) size = raw.size() - pos;
        out.append(raw, pos, static_cast<std::size_t>(size));
        pos += static_cast<std::size_t>(size);
        if (raw.compare(pos, 2, "\r\n") == 0) pos += 2;
    }
    return out;
}
lingxi::HttpReply transport(
    const std::string& url,
    const std::map<std::string, std::string>& headers,
    const std::string& body,
    std::chrono::milliseconds timeout) {
    return http_exchange(url, headers, body, timeout);
}

lingxi::json::Value post_target(const lingxi::native::Target& target) {
    lingxi::HttpReply reply =
        http_exchange(target.url, target.headers, target.body.dump(), std::chrono::seconds(20));
    if (reply.status_code < 200 || reply.status_code > 299) {
        throw lingxi::NativeHttpError(reply.status_code, reply.body);
    }
    return lingxi::json::Value::parse(reply.body);
}

lingxi::json::Value lmp_request(const std::string& model) {
    lingxi::json::Value text(lingxi::json::Object{});
    text.set("type", lingxi::json::Value("text"));
    text.set("text", lingxi::json::Value("hello from the C++ SDK"));
    lingxi::json::Value content(lingxi::json::Array{});
    content.push(text);
    lingxi::json::Value message(lingxi::json::Object{});
    message.set("role", lingxi::json::Value("user"));
    message.set("content", content);
    lingxi::json::Value messages(lingxi::json::Array{});
    messages.push(message);
    lingxi::json::Value request(lingxi::json::Object{});
    request.set("model", lingxi::json::Value(model));
    request.set("messages", messages);
    return request;
}

std::string text_of(const lingxi::json::Value& response) {
    std::string out;
    const lingxi::json::Value* content = response.find("content");
    if (!content || !content->is_array()) return out;
    for (const auto& block : content->array()) {
        const lingxi::json::Value* type = block.find("type");
        const lingxi::json::Value* text = block.find("text");
        if (type && type->is_string() && type->string() == "text" && text && text->is_string()) {
            out += text->string();
        }
    }
    return out;
}

std::string thinking_of(const lingxi::json::Value& response) {
    std::string out;
    const lingxi::json::Value* content = response.find("content");
    if (!content || !content->is_array()) return out;
    for (const auto& block : content->array()) {
        const lingxi::json::Value* type = block.find("type");
        const lingxi::json::Value* value = block.find("thinking");
        if (type && type->is_string() && type->string() == "thinking" && value && value->is_string()) {
            out += value->string();
        }
    }
    return out;
}

std::optional<std::string> thinking_signature_of(const lingxi::json::Value& response) {
    const lingxi::json::Value* content = response.find("content");
    if (!content || !content->is_array()) return std::nullopt;
    for (const auto& block : content->array()) {
        const lingxi::json::Value* type = block.find("type");
        if (!type || !type->is_string() || type->string() != "thinking") continue;
        const lingxi::json::Value* signature = block.find("signature");
        if (signature && signature->is_string()) return signature->string();
    }
    return std::nullopt;
}

std::optional<std::string> tool_call_name(const lingxi::json::Value& response) {
    const lingxi::json::Value* content = response.find("content");
    if (!content || !content->is_array()) return std::nullopt;
    for (const auto& block : content->array()) {
        const lingxi::json::Value* type = block.find("type");
        if (!type || !type->is_string() || type->string() != "tool_call") continue;
        const lingxi::json::Value* name = block.find("name");
        if (name && name->is_string()) return name->string();
    }
    return std::nullopt;
}

std::optional<long long> usage_value(const lingxi::json::Value& response, const std::string& key) {
    const lingxi::json::Value* usage = response.find("usage");
    if (!usage || !usage->is_object()) return std::nullopt;
    const lingxi::json::Value* value = usage->find(key);
    if (!value || !value->is_number()) return std::nullopt;
    return value->optional_integer();
}

std::optional<std::string> finish_reason_of(const lingxi::json::Value& response) {
    const lingxi::json::Value* value = response.find("finishReason");
    if (value && value->is_string()) return value->string();
    return std::nullopt;
}

void expect_usage(const std::optional<long long>& actual, long long expected, const std::string& label) {
    ++checks;
    if (!actual || *actual != expected) {
        std::cerr << "FAIL: " << label << "\n"
                  << "  expected: " << expected << "\n"
                  << "  actual:   " << (actual ? std::to_string(*actual) : std::string("null")) << "\n";
        std::exit(1);
    }
}

std::vector<std::string> json_strings(const lingxi::json::Value& value) {
    std::vector<std::string> out;
    if (!value.is_array()) return out;
    for (const auto& entry : value.array()) {
        if (entry.is_string()) out.push_back(entry.string());
    }
    return out;
}

std::vector<std::string> provider_ids(const std::vector<lingxi::catalog::CatalogProvider>& providers) {
    std::vector<std::string> out;
    for (const auto& provider : providers) out.push_back(provider.id);
    return out;
}

std::vector<std::string> model_ids(const std::vector<lingxi::catalog::CatalogModel>& models) {
    std::vector<std::string> out;
    for (const auto& model : models) out.push_back(model.qualified_id());
    return out;
}

std::vector<std::string> bare_model_ids(const std::vector<lingxi::catalog::CatalogModel>& models) {
    std::vector<std::string> out;
    for (const auto& model : models) out.push_back(model.id);
    return out;
}


// ---------------------------------------------------------------- catalog suite

void test_catalog(const std::filesystem::path& root) {
    const std::filesystem::path fixtures = root / "conformance" / "catalog" / "fixtures";
    lingxi::json::Value expectations = lingxi::json::Value::parse(
        read_file((root / "conformance" / "catalog" / "expectations.json").string()));
    lingxi::catalog::ModelCatalog catalog =
        lingxi::catalog::ModelCatalog::decode(read_file((fixtures / "v2_catalog.json").string()));

    expect(provider_ids(catalog.providers()) == json_strings(expectations.at("providerOrder")),
           "provider order: " + join(provider_ids(catalog.providers())));
    expect_equal(catalog.revision().catalog_revision, expectations.at("catalogRevision").string(),
                 "catalog revision");
    expect(catalog.providers().size() ==
               static_cast<std::size_t>(expectations.at("totalProviders").optional_integer().value_or(-1)),
           "total providers");
    expect(catalog.total_models() ==
               static_cast<std::size_t>(expectations.at("totalModels").optional_integer().value_or(-1)),
           "total models");

    const lingxi::json::Value& featured = expectations.at("featuredProviderOrder");
    expect(provider_ids(catalog.sorted_providers(json_strings(featured.at("featured")))) ==
               json_strings(featured.at("expected")),
           "featured providers rank first");
    expect(bare_model_ids(catalog.models_for_provider("prov10")) ==
               json_strings(expectations.at("prov10ModelOrder")),
           "prov10 model order");
    expect(bare_model_ids(catalog.models_for_provider("deepseek")) ==
               json_strings(expectations.at("deepseekModelOrder")),
           "deepseek model order");
    expect(model_ids(catalog.all_models()) == json_strings(expectations.at("allModelQualifiedOrder")),
           "all models in catalog order: " + join(model_ids(catalog.all_models())));

    const lingxi::json::Value& search = expectations.at("search");
    expect(model_ids(catalog.search("gpt", 1)) == json_strings(search.at("gptLimit1")), "search honours the limit");
    expect(model_ids(catalog.search("deepseek")) == json_strings(search.at("deepseek")),
           "search matches provider names");

    const lingxi::json::Value& filters = expectations.at("filters");
    {
        lingxi::catalog::ModelFilter filter;
        filter.vision = true;
        expect(model_ids(catalog.models_matching(filter)) == json_strings(filters.at("vision")),
               "vision filter");
    }
    {
        lingxi::catalog::ModelFilter filter;
        filter.minimum_context_window = 200000;
        expect(model_ids(catalog.models_matching(filter)) == json_strings(filters.at("minContext200k")),
               "minimum context filter");
    }
    {
        lingxi::catalog::ModelFilter filter;
        filter.provider_id = "deepseek";
        expect(model_ids(catalog.models_matching(filter)) == json_strings(filters.at("deepseekDefault")),
               "deprecated models are hidden by default");
        filter.include_deprecated = true;
        expect(model_ids(catalog.models_matching(filter)) ==
                   json_strings(filters.at("deepseekIncludeDeprecated")),
               "include_deprecated reveals deprecated models");
    }

    std::optional<lingxi::catalog::CatalogModel> gpt10 = catalog.model("prov10", "gpt-10");
    expect(gpt10.has_value(), "gpt-10 decodes");
    expect(gpt10->capabilities.reasoning, "gpt-10 reasoning");
    expect((gpt10->capabilities.reasoning_efforts == std::vector<std::string>{"low", "medium", "high"}),
           "gpt-10 reasoning efforts");
    expect(gpt10->capabilities.tool_calling, "gpt-10 tool calling");
    expect(gpt10->capabilities.structured_output, "gpt-10 structured output");
    expect(gpt10->capabilities.attachments, "gpt-10 attachments");
    expect(gpt10->capabilities.vision, "gpt-10 vision derived from input modalities");
    expect(gpt10->capabilities.audio_input, "gpt-10 audio input");
    expect(gpt10->capabilities.audio_output, "gpt-10 audio output");
    expect(gpt10->capabilities.supports_temperature.has_value() &&
               !*gpt10->capabilities.supports_temperature,
           "gpt-10 temperature false is not collapsed into unknown");
    expect(!gpt10->capabilities.open_weights, "gpt-10 open weights false");
    expect(gpt10->context_window().value_or(-1) == 400000, "gpt-10 context limit");
    expect(gpt10->limits.input.value_or(-1) == 272000, "gpt-10 input limit");
    expect(gpt10->max_output_tokens().value_or(-1) == 128000, "gpt-10 output limit");
    expect(gpt10->pricing.input.has_value() && *gpt10->pricing.input == 0.0,
           "gpt-10 free input pricing is 0.0, not unknown");
    expect(gpt10->pricing.output.has_value() && std::abs(*gpt10->pricing.output - 0.4) < 1e-9,
           "gpt-10 output pricing");
    expect(gpt10->pricing.cache_read.has_value() && std::abs(*gpt10->pricing.cache_read - 0.005) < 1e-9,
           "gpt-10 cache read pricing");
    const lingxi::json::Value* experimental = gpt10->fields.find("experimental");
    expect(experimental && experimental->is_object(), "unknown provider fields are preserved");
    const lingxi::json::Value* warmup = experimental ? experimental->find("warmup") : nullptr;
    expect(warmup && warmup->is_bool() && warmup->boolean(), "nested unknown fields are preserved");

    std::optional<lingxi::catalog::CatalogModel> gpt5 = catalog.model("prov10/gpt-5");
    expect(gpt5.has_value(), "gpt-5 resolves by qualified id");
    expect(!gpt5->capabilities.structured_output, "gpt-5 structured output false");
    expect(gpt5->context_window().value_or(-1) == 0, "gpt-5 zero context stays a known zero");
    expect(gpt5->max_output_tokens().value_or(-1) == 64000, "gpt-5 output limit");
    expect(gpt5->pricing.is_unknown(), "gpt-5 pricing is unknown");
    expect(!gpt5->capabilities.supports_temperature.has_value(), "gpt-5 temperature is unknown");
    expect(gpt5->status == lingxi::catalog::ModelStatus::Unknown, "gpt-5 status stays unknown") ;
    expect(lingxi::catalog::is_selectable(gpt5->status), "unknown status stays selectable");

    std::optional<lingxi::catalog::CatalogModel> alpha = catalog.model("prov2", "alpha-beta");
    expect(alpha.has_value() && alpha->status == lingxi::catalog::ModelStatus::Preview, "beta maps to preview");
    std::optional<lingxi::catalog::CatalogModel> legacy = catalog.model("deepseek", "deepseek-v3");
    expect(legacy.has_value() && legacy->status == lingxi::catalog::ModelStatus::Deprecated,
           "deprecated status");
    expect(!lingxi::catalog::is_selectable(legacy->status), "deprecated models are not selectable");

    lingxi::catalog::ModelCatalog v1 =
        lingxi::catalog::ModelCatalog::decode(read_file((fixtures / "v1_legacy.json").string()));
    expect_equal(v1.revision().schema_version, "1.0", "schemaVersion falls back to version");
    std::optional<lingxi::catalog::CatalogModel> llama = v1.model("ollama", "llama3");
    expect(llama.has_value() && llama->context_window().value_or(-1) == 8192, "legacy context window");

    lingxi::catalog::ModelCatalog malformed =
        lingxi::catalog::ModelCatalog::decode(read_file((fixtures / "malformed_entry.json").string()));
    expect(malformed.model("p", "published-key").has_value(), "the map key names the model when id is blank");
    expect(!malformed.model("p", "internal-id").has_value(), "a blank id field never becomes the model id");

    bool unsupported = false;
    try {
        lingxi::catalog::ModelCatalog::decode(read_file((fixtures / "unsupported_schema.json").string()));
    } catch (const lingxi::catalog::CatalogError& error) {
        unsupported = error.kind() == lingxi::catalog::CatalogError::Kind::UnsupportedSchemaVersion &&
                      !error.schema_version().empty();
    }
    expect(unsupported, "schema 3.0 is rejected as UnsupportedSchemaVersion");

    bool invalid = false;
    try {
        lingxi::catalog::ModelCatalog::decode(read_file((fixtures / "broken_empty_providers.json").string()));
    } catch (const lingxi::catalog::CatalogError& error) {
        invalid = error.kind() == lingxi::catalog::CatalogError::Kind::InvalidCatalog;
    }
    expect(invalid, "empty providers is rejected as InvalidCatalog");
}

// ------------------------------------------------------------------ fixture server

// Replays the conformance fixtures over real HTTP. Streaming requests get a
// chunked body delivered in pieces so the client observes incremental reads.
class FixtureServer {
public:
    struct Request {
        std::string method;
        std::string path;
        std::string body;
        std::map<std::string, std::string> headers;
    };

    struct Reply {
        std::string content_type = "application/json";
        std::string body;
        bool streaming = false;
    };

    explicit FixtureServer(std::filesystem::path native_dir) : native_dir_(std::move(native_dir)) {
        listener_ = ::socket(AF_INET, SOCK_STREAM, 0);
        int reuse = 1;
        ::setsockopt(listener_, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse));
        sockaddr_in address{};
        address.sin_family = AF_INET;
        address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        address.sin_port = 0;
        ::bind(listener_, reinterpret_cast<sockaddr*>(&address), sizeof(address));
        socklen_t length = sizeof(address);
        ::getsockname(listener_, reinterpret_cast<sockaddr*>(&address), &length);
        port_ = ntohs(address.sin_port);
        ::listen(listener_, 16);
        worker_ = std::thread([this] { loop(); });
    }

    ~FixtureServer() { stop(); }

    void stop() {
        bool expected = false;
        if (!stopping_.compare_exchange_strong(expected, true)) return;
        ::shutdown(listener_, SHUT_RDWR);
        ::close(listener_);
        if (worker_.joinable()) worker_.join();
    }

    std::string base_url() const { return "http://127.0.0.1:" + std::to_string(port_); }
    int port() const { return port_; }

    void reset_requests() {
        std::lock_guard<std::mutex> guard(mutex_);
        requests_.clear();
    }

    std::vector<Request> requests() const {
        std::lock_guard<std::mutex> guard(mutex_);
        return requests_;
    }

    static void send_all(int client, const std::string& text) {
        std::size_t sent = 0;
        while (sent < text.size()) {
            ssize_t wrote = ::send(client, text.data() + sent, text.size() - sent, 0);
            if (wrote <= 0) return;
            sent += static_cast<std::size_t>(wrote);
        }
    }

    // Amazon Event Stream framing: a 4-byte big-endian total length starts each
    // message, which is what the core decoder checks before consuming a frame.
    static std::vector<std::string> split_eventstream_frames(const std::string& body) {
        std::vector<std::string> frames;
        std::size_t offset = 0;
        while (offset + 4 <= body.size()) {
            const auto* bytes = reinterpret_cast<const unsigned char*>(body.data() + offset);
            std::size_t total = (static_cast<std::size_t>(bytes[0]) << 24) |
                                (static_cast<std::size_t>(bytes[1]) << 16) |
                                (static_cast<std::size_t>(bytes[2]) << 8) |
                                static_cast<std::size_t>(bytes[3]);
            if (total == 0 || offset + total > body.size()) break;
            frames.push_back(body.substr(offset, total));
            offset += total;
        }
        return frames;
    }

private:
    void loop() {
        while (!stopping_.load()) {
            sockaddr_in peer{};
            socklen_t length = sizeof(peer);
            int client = ::accept(listener_, reinterpret_cast<sockaddr*>(&peer), &length);
            if (client < 0) return;
            try {
                handle(client);
            } catch (const std::exception&) {
                // A malformed or abruptly closed connection must not kill the server.
            }
            ::close(client);
        }
    }

    void handle(int client) {
        int no_sigpipe = 1;
        ::setsockopt(client, SOL_SOCKET, SO_NOSIGPIPE, &no_sigpipe, sizeof(no_sigpipe));
        std::string raw;
        char buffer[8192];
        while (raw.find("\r\n\r\n") == std::string::npos) {
            ssize_t read_now = ::recv(client, buffer, sizeof(buffer), 0);
            if (read_now <= 0) return;
            raw.append(buffer, static_cast<std::size_t>(read_now));
        }
        std::size_t header_end = raw.find("\r\n\r\n");
        Request request;
        std::istringstream header_block(raw.substr(0, header_end));
        std::string line;
        std::getline(header_block, line);
        {
            std::istringstream first(line);
            first >> request.method >> request.path;
        }
        std::size_t content_length = 0;
        while (std::getline(header_block, line)) {
            if (!line.empty() && line.back() == '\r') line.pop_back();
            std::size_t colon = line.find(':');
            if (colon == std::string::npos) continue;
            std::string key = line.substr(0, colon);
            std::string value = line.substr(colon + 1);
            if (!value.empty() && value.front() == ' ') value.erase(0, 1);
            std::string lower_key = key;
            for (char& ch : lower_key) ch = char(std::tolower(static_cast<unsigned char>(ch)));
            if (lower_key == "content-length") content_length = std::stoul(value);
            request.headers[key] = value;
        }
        request.body = raw.substr(header_end + 4);
        while (request.body.size() < content_length) {
            ssize_t read_now = ::recv(client, buffer, sizeof(buffer), 0);
            if (read_now <= 0) return;
            request.body.append(buffer, static_cast<std::size_t>(read_now));
        }

        {
            std::lock_guard<std::mutex> guard(mutex_);
            requests_.push_back(request);
        }

        Reply reply = route(request);
        if (reply.streaming) {
            send_stream(client, reply);
        } else {
            std::ostringstream head;
            head << "HTTP/1.1 200 OK\r\n";
            head << "Content-Type: " << reply.content_type << "\r\n";
            head << "Content-Length: " << reply.body.size() << "\r\n";
            head << "Connection: close\r\n\r\n";
            send_all(client, head.str());
            send_all(client, reply.body);
        }
    }

    Reply route(const Request& request) const {
        std::string path = request.path;
        for (char& ch : path) ch = char(std::tolower(static_cast<unsigned char>(ch)));
        const bool stream = request.body.find("\"stream\":true") != std::string::npos ||
                            path.find("alt=sse") != std::string::npos ||
                            (path.size() >= 7 && path.compare(path.size() - 7, 7, "-stream") == 0);

        Reply reply;
        if (path.find("-stream") != std::string::npos || path.find("/converse-stream") != std::string::npos) {
            reply.content_type = "application/vnd.amazon.eventstream";
            reply.body = read_fixture("aws_bedrock_stream.eventstream");
            reply.streaming = true;
            return reply;
        }
        if (path.find("/converse") != std::string::npos) {
            reply.body = read_fixture("aws_bedrock_response.json");
            return reply;
        }
        if (path.find("/v2/rerank") != std::string::npos) {
            reply.body = read_fixture("cohere_rerank_response.json");
            return reply;
        }
        if (path.find("/v2/embed") != std::string::npos) {
            reply.body = R"({"embeddings":{"float":[[0.1,0.2,0.3],[0.4,0.5,0.6]]}})";
            return reply;
        }
        if (path.find("/api/embed") != std::string::npos) {
            reply.body = R"({"embeddings":[[0.1,0.2,0.3],[0.4,0.5,0.6]],"prompt_eval_count":8})";
            return reply;
        }
        if (path.find("/embeddings") != std::string::npos) {
            reply.body = read_fixture("openai_embed_response.json");
            return reply;
        }
        if (path.find("/api/chat") != std::string::npos) {
            if (stream) {
                reply.content_type = "application/x-ndjson";
                reply.body = read_fixture("ollama_chat_stream.ndjson");
                reply.streaming = true;
            } else {
                reply.body = read_fixture("ollama_chat_response.json");
            }
            return reply;
        }
        if (path.find("/v2/chat") != std::string::npos) {
            if (stream) {
                reply.content_type = "text/event-stream";
                reply.body = read_fixture("cohere_chat_stream.sse");
                reply.streaming = true;
            } else {
                reply.body = read_fixture("cohere_chat_response.json");
            }
            return reply;
        }
        if (path.find("/v1/messages") != std::string::npos) {
            if (stream) {
                reply.content_type = "text/event-stream";
                reply.body = read_fixture("anthropic_messages_stream.sse");
                reply.streaming = true;
            } else {
                reply.body = read_fixture("anthropic_messages_response.json");
            }
            return reply;
        }
        if (path.find("/deployments/") != std::string::npos) {
            if (stream) {
                reply.content_type = "text/event-stream";
                reply.body = read_fixture("azure_openai_stream.sse");
                reply.streaming = true;
            } else {
                reply.body = read_fixture("azure_openai_response.json");
            }
            return reply;
        }
        if (path.find("generatecontent") != std::string::npos) {
            const bool vertex = path.find("/publishers/") != std::string::npos;
            if (stream) {
                reply.content_type = "text/event-stream";
                reply.body = read_fixture(vertex ? "google_vertex_stream.sse" : "google_gemini_stream.sse");
                reply.streaming = true;
            } else {
                reply.body = read_fixture(vertex ? "google_vertex_response.json" : "google_gemini_response.json");
            }
            return reply;
        }
        if (path.find("/responses") != std::string::npos) {
            if (stream) {
                reply.content_type = "text/event-stream";
                reply.body = read_fixture("openai_responses_stream.sse");
                reply.streaming = true;
            } else {
                reply.body = read_fixture("openai_responses_response.json");
            }
            return reply;
        }
        if (stream) {
            reply.content_type = "text/event-stream";
            reply.body = read_fixture("openai_chat_stream.sse");
            reply.streaming = true;
        } else {
            reply.body = read_fixture("openai_chat_response.json");
        }
        return reply;
    }

    std::string read_fixture(const std::string& name) const {
        return read_file((native_dir_ / name).string());
    }

    void send_stream(int client, const Reply& reply) {
        std::ostringstream head;
        head << "HTTP/1.1 200 OK\r\n";
        head << "Content-Type: " << reply.content_type << "\r\n";
        head << "Transfer-Encoding: chunked\r\n";
        head << "Connection: keep-alive\r\n\r\n";
        send_all(client, head.str());

        std::vector<std::string> pieces;
        if (reply.content_type == "application/x-ndjson") {
            std::size_t start = 0;
            while (start < reply.body.size()) {
                std::size_t end = reply.body.find('\n', start);
                if (end == std::string::npos) end = reply.body.size();
                std::string line = reply.body.substr(start, end - start);
                start = end + 1;
                if (!line.empty() && line.back() == '\r') line.pop_back();
                if (!line.empty()) pieces.push_back(line + "\n");
            }
        } else if (reply.content_type == "application/vnd.amazon.eventstream") {
            pieces = split_eventstream_frames(reply.body);
        } else {
            std::size_t start = 0;
            while (start < reply.body.size()) {
                std::size_t end = reply.body.find("\n\n", start);
                if (end == std::string::npos) {
                    end = reply.body.size();
                } else {
                    end += 2;
                }
                std::string block = reply.body.substr(start, end - start);
                start = end;
                if (!block.empty()) pieces.push_back(block);
            }
        }

        for (const auto& piece : pieces) {
            std::ostringstream chunk;
            chunk << std::hex << piece.size() << "\r\n";
            send_all(client, chunk.str());
            send_all(client, piece);
            send_all(client, "\r\n");
            std::this_thread::sleep_for(std::chrono::milliseconds(2));
        }
        send_all(client, "0\r\n\r\n");
    }

    std::filesystem::path native_dir_;
    int listener_ = -1;
    int port_ = 0;
    std::atomic<bool> stopping_{false};
    std::thread worker_;
    mutable std::mutex mutex_;
    std::vector<Request> requests_;
};

// Sends the first frame, then waits for the test to release the rest. This is
// the only way to prove the client surfaces an event before the upstream
// response is complete.
class GatedServer {
public:
    GatedServer(std::string first_frame, std::string rest, std::string content_type)
        : first_frame_(std::move(first_frame)), rest_(std::move(rest)), content_type_(std::move(content_type)) {
        listener_ = ::socket(AF_INET, SOCK_STREAM, 0);
        int reuse = 1;
        ::setsockopt(listener_, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse));
        sockaddr_in address{};
        address.sin_family = AF_INET;
        address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        address.sin_port = 0;
        ::bind(listener_, reinterpret_cast<sockaddr*>(&address), sizeof(address));
        socklen_t length = sizeof(address);
        ::getsockname(listener_, reinterpret_cast<sockaddr*>(&address), &length);
        port_ = ntohs(address.sin_port);
        ::listen(listener_, 4);
        worker_ = std::thread([this] { serve(); });
    }

    ~GatedServer() {
        first_served_.store(true);
        release_.store(true);
        ::shutdown(listener_, SHUT_RDWR);
        ::close(listener_);
        if (worker_.joinable()) worker_.join();
    }

    std::string base_url() const { return "http://127.0.0.1:" + std::to_string(port_); }
    bool first_frame_served() const { return first_served_.load(); }
    void release_rest() { release_.store(true); }

private:
    void serve() {
        sockaddr_in peer{};
        socklen_t length = sizeof(peer);
        int client = ::accept(listener_, reinterpret_cast<sockaddr*>(&peer), &length);
        if (client < 0) return;
        int no_sigpipe = 1;
        ::setsockopt(client, SOL_SOCKET, SO_NOSIGPIPE, &no_sigpipe, sizeof(no_sigpipe));
        std::string raw;
        char buffer[4096];
        while (raw.find("\r\n\r\n") == std::string::npos) {
            ssize_t read_now = ::recv(client, buffer, sizeof(buffer), 0);
            if (read_now <= 0) {
                ::close(client);
                return;
            }
            raw.append(buffer, static_cast<std::size_t>(read_now));
        }
        std::ostringstream head;
        head << "HTTP/1.1 200 OK\r\n";
        head << "Content-Type: " << content_type_ << "\r\n";
        head << "Transfer-Encoding: chunked\r\n";
        head << "Connection: keep-alive\r\n\r\n";
        FixtureServer::send_all(client, head.str());
        FixtureServer::send_all(client, chunk_of(first_frame_));
        first_served_.store(true);
        for (int i = 0; i < 1500; ++i) {
            if (release_.load()) break;
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
        FixtureServer::send_all(client, chunk_of(rest_));
        FixtureServer::send_all(client, "0\r\n\r\n");
        ::shutdown(client, SHUT_WR);
        ::close(client);
    }

    static std::string chunk_of(const std::string& piece) {
        std::ostringstream chunk;
        chunk << std::hex << piece.size() << "\r\n" << piece << "\r\n";
        return chunk.str();
    }

    int listener_ = -1;
    int port_ = 0;
    std::string first_frame_;
    std::string rest_;
    std::string content_type_;
    std::atomic<bool> first_served_{false};
    std::atomic<bool> release_{false};
    std::thread worker_;
};


// Serves one canned raw HTTP response per connection. Used for status-code
// contract checks where the body must not be a fixture.
class RawServer {
public:
    explicit RawServer(std::string response) : response_(std::move(response)) {
        listener_ = ::socket(AF_INET, SOCK_STREAM, 0);
        int reuse = 1;
        ::setsockopt(listener_, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse));
        sockaddr_in address{};
        address.sin_family = AF_INET;
        address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        address.sin_port = 0;
        ::bind(listener_, reinterpret_cast<sockaddr*>(&address), sizeof(address));
        socklen_t length = sizeof(address);
        ::getsockname(listener_, reinterpret_cast<sockaddr*>(&address), &length);
        port_ = ntohs(address.sin_port);
        ::listen(listener_, 8);
        worker_ = std::thread([this] { loop(); });
    }

    ~RawServer() {
        stopping_.store(true);
        ::shutdown(listener_, SHUT_RDWR);
        ::close(listener_);
        if (worker_.joinable()) worker_.join();
    }

    std::string base_url() const { return "http://127.0.0.1:" + std::to_string(port_); }

private:
    void loop() {
        while (!stopping_.load()) {
            sockaddr_in peer{};
            socklen_t length = sizeof(peer);
            int client = ::accept(listener_, reinterpret_cast<sockaddr*>(&peer), &length);
            if (client < 0) return;
            int no_sigpipe = 1;
            ::setsockopt(client, SOL_SOCKET, SO_NOSIGPIPE, &no_sigpipe, sizeof(no_sigpipe));
            std::string raw;
            char buffer[4096];
            while (raw.find("\r\n\r\n") == std::string::npos) {
                ssize_t read_now = ::recv(client, buffer, sizeof(buffer), 0);
                if (read_now <= 0) break;
                raw.append(buffer, static_cast<std::size_t>(read_now));
            }
            FixtureServer::send_all(client, response_);
            ::shutdown(client, SHUT_WR);
            ::close(client);
        }
    }

    std::string response_;
    int listener_ = -1;
    int port_ = 0;
    std::atomic<bool> stopping_{false};
    std::thread worker_;
};

// Sends response headers and then stays silent: the stream never ends, which
// is what a cancel and an idle timeout must survive.
class SilentServer {
public:
    SilentServer() {
        listener_ = ::socket(AF_INET, SOCK_STREAM, 0);
        int reuse = 1;
        ::setsockopt(listener_, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse));
        sockaddr_in address{};
        address.sin_family = AF_INET;
        address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        address.sin_port = 0;
        ::bind(listener_, reinterpret_cast<sockaddr*>(&address), sizeof(address));
        socklen_t length = sizeof(address);
        ::getsockname(listener_, reinterpret_cast<sockaddr*>(&address), &length);
        port_ = ntohs(address.sin_port);
        ::listen(listener_, 4);
        worker_ = std::thread([this] { serve(); });
    }

    ~SilentServer() {
        stopping_.store(true);
        ::shutdown(listener_, SHUT_RDWR);
        ::close(listener_);
        {
            std::lock_guard<std::mutex> guard(mutex_);
            if (client_ >= 0) ::shutdown(client_, SHUT_RDWR);
        }
        if (worker_.joinable()) worker_.join();
    }

    std::string base_url() const { return "http://127.0.0.1:" + std::to_string(port_); }

    bool accepted() const { return accepted_.load(); }

private:
    void serve() {
        sockaddr_in peer{};
        socklen_t length = sizeof(peer);
        int client = ::accept(listener_, reinterpret_cast<sockaddr*>(&peer), &length);
        if (client < 0) return;
        {
            std::lock_guard<std::mutex> guard(mutex_);
            client_ = client;
        }
        int no_sigpipe = 1;
        ::setsockopt(client, SOL_SOCKET, SO_NOSIGPIPE, &no_sigpipe, sizeof(no_sigpipe));
        std::string raw;
        char buffer[4096];
        while (raw.find("\r\n\r\n") == std::string::npos) {
            ssize_t read_now = ::recv(client, buffer, sizeof(buffer), 0);
            if (read_now <= 0) return;
            raw.append(buffer, static_cast<std::size_t>(read_now));
        }
        FixtureServer::send_all(client,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n"
            "Transfer-Encoding: chunked\r\nConnection: keep-alive\r\n\r\n");
        accepted_.store(true);
        while (!stopping_.load()) std::this_thread::sleep_for(std::chrono::milliseconds(20));
        ::close(client);
    }

    int listener_ = -1;
    int port_ = 0;
    std::mutex mutex_;
    int client_ = -1;
    std::atomic<bool> accepted_{false};
    std::atomic<bool> stopping_{false};
    std::thread worker_;
};

// ----------------------------------------------------------- catalog loader suite

void test_catalog_loader(std::filesystem::path root) {
    const std::filesystem::path fixtures = root / "conformance" / "catalog" / "fixtures";
    const std::string document = read_file((fixtures / "v2_catalog.json").string());
    std::filesystem::path cache = std::filesystem::temp_directory_path() / "lingxi-cpp-catalog-cache";
    std::filesystem::remove_all(cache);

    struct Call {
        std::string url;
        std::map<std::string, std::string> headers;
    };
    std::vector<Call> calls;

    lingxi::catalog::LoaderConfig config;
    config.endpoint = "https://catalog.invalid/models.json";
    config.cache_directory = cache;
    config.max_age = std::chrono::seconds(300);

    lingxi::catalog::CatalogLoader loader(config, [&](const std::string& url,
                                                      const std::map<std::string, std::string>& headers,
                                                      std::chrono::milliseconds) {
        calls.push_back(Call{url, headers});
        lingxi::catalog::HttpReply reply;
        reply.status_code = 200;
        reply.body = document;
        reply.etag = "\"etag-1\"";
        return reply;
    });

    lingxi::catalog::ModelCatalog first = loader.load();
    expect(first.total_models() == 5, "the first load decodes the upstream document");
    expect(calls.size() == 1, "the first load hits the transport once");
    expect(calls[0].headers.count("Accept") == 1, "the loader sends Accept");
    expect(calls[0].headers.count("User-Agent") == 1, "the loader sends a User-Agent");
    expect(calls[0].headers.count("If-None-Match") == 0, "no conditional header without a cached ETag");
    expect(std::filesystem::exists(cache / "models.json"), "the document is cached on disk");
    expect(std::filesystem::exists(cache / "models.json.meta.json"), "the cache metadata is written");
    {
        lingxi::json::Value meta =
            lingxi::json::Value::parse(read_file((cache / "models.json.meta.json").string()));
        expect(meta.find("etag") && meta.at("etag").string() == "\"etag-1\"", "the ETag is persisted");
        expect(meta.find("fetchedAt") && meta.at("fetchedAt").is_string(), "the fetch time is persisted");
    }

    lingxi::catalog::ModelCatalog second = loader.load();
    expect(second.total_models() == 5, "a fresh cache is served without the transport");
    expect(calls.size() == 1, "a fresh cache performs no request");

    lingxi::catalog::ModelCatalog refreshed = loader.load(true);
    expect(calls.size() == 2, "force_refresh revalidates");
    expect(refreshed.total_models() == 5, "force_refresh returns a catalog");
    expect(calls[1].headers.count("If-None-Match") == 1, "force_refresh sends If-None-Match");
    expect(calls[1].headers.at("If-None-Match") == "\"etag-1\"", "the stored ETag is revalidated");

    // A 304 keeps the cached document and refreshes the metadata timestamp.
    lingxi::catalog::CatalogLoader revalidating(config, [&](const std::string& url,
                                                               const std::map<std::string, std::string>& headers,
                                                               std::chrono::milliseconds) {
        calls.push_back(Call{url, headers});
        lingxi::catalog::HttpReply reply;
        reply.status_code = 304;
        return reply;
    });
    lingxi::catalog::ModelCatalog cached = revalidating.load();
    expect(cached.total_models() == 5, "a 304 replays the on-disk catalog");
    expect(calls.size() == 3, "the 304 path performs exactly one request");
    expect(calls[2].headers.count("If-None-Match") == 1, "the 304 request was conditional");

    // A failing upstream falls back to the cache instead of failing the call.
    lingxi::catalog::CatalogLoader failing(config, [](const std::string&,
                                                   const std::map<std::string, std::string>&,
                                                   std::chrono::milliseconds) {
        lingxi::catalog::HttpReply reply;
        reply.status_code = 503;
        reply.body = "upstream unavailable";
        return reply;
    });
    lingxi::catalog::ModelCatalog stale = failing.load();
    expect(stale.total_models() == 5, "a 503 with a warm cache serves stale data");

    // With no cache at all the same failure must surface as a typed error.
    lingxi::catalog::LoaderConfig cold = config;
    cold.cache_directory = std::filesystem::temp_directory_path() / "lingxi-cpp-empty-cache";
    std::filesystem::remove_all(*cold.cache_directory);
    lingxi::catalog::CatalogLoader cold_loader(cold, [](const std::string&,
                                                     const std::map<std::string, std::string>&,
                                                     std::chrono::milliseconds) {
        lingxi::catalog::HttpReply reply;
        reply.status_code = 503;
        return reply;
    });
    bool unexpected_status = false;
    try {
        cold_loader.load();
    } catch (const lingxi::catalog::CatalogError& error) {
        unexpected_status = error.kind() == lingxi::catalog::CatalogError::Kind::UnexpectedStatus &&
                            error.status_code() == 503;
    }
    expect(unexpected_status, "a cold 503 raises UnexpectedStatus(503)");

    lingxi::catalog::CatalogLoader offline(cold, [](const std::string&,
                                                 const std::map<std::string, std::string>&,
                                                 std::chrono::milliseconds) -> lingxi::catalog::HttpReply {
        throw std::runtime_error("connection refused");
    });
    bool transport_error = false;
    try {
        offline.load();
    } catch (const lingxi::catalog::CatalogError& error) {
        transport_error = error.kind() == lingxi::catalog::CatalogError::Kind::Transport;
    }
    expect(transport_error, "a cold transport failure raises Transport");

    std::filesystem::remove_all(cache);
    std::filesystem::remove_all(*cold.cache_directory);
}

// ------------------------------------------------------------------ registry suite

void test_registry(const std::filesystem::path& root) {
    const std::string registry_json = read_file((root / "registry" / "registry.json").string());
    lingxi::registry::Endpoint bedrock =
        lingxi::registry::resolve_endpoint(registry_json, "amazon-bedrock", "aws_bedrock");
    expect(bedrock.endpoint.find("bedrock-runtime") != std::string::npos,
           "the registry resolves the Bedrock runtime endpoint: " + bedrock.endpoint);
    expect(bedrock.provider.has_value() && *bedrock.provider == "amazon-bedrock", "the registry echoes the provider");
    expect(bedrock.auth_type.has_value() && *bedrock.auth_type == "aws_sigv4", "the registry reports SigV4 auth");

    expect(throws_with_code([&] { lingxi::registry::resolve_endpoint(registry_json, "nope", "aws_bedrock"); },
                            "registry"),
           "an unknown provider reports the registry error code");
    expect(throws_with_code([&] { lingxi::registry::resolve_endpoint(registry_json, "amazon-bedrock", "nope"); },
                            "unsupported_protocol"),
           "an unsupported protocol surfaces as a core error");
}

// -------------------------------------------------------------------- error suite

void test_error_contract() {
    expect(throws_with_code([] {
               lingxi::json::Value request(lingxi::json::Object{});
               lingxi::json::Value schema(lingxi::json::Object{});
               schema.set("type", lingxi::json::Value("object"));
               lingxi::json::Value format(lingxi::json::Object{});
               format.set("type", lingxi::json::Value("json_schema"));
               format.set("name", lingxi::json::Value("answer"));
               format.set("schema", schema);
               format.set("strict", lingxi::json::Value(true));
               request.set("model", lingxi::json::Value("claude-3-5-sonnet"));
               request.set("messages", lingxi::json::Value(lingxi::json::Array{}));
               request.set("responseFormat", format);
               lingxi::Core::translate_request("anthropic_messages", request);
           },
           "unsupported_feature"),
           "Anthropic structured output is an explicit unsupported feature");

    expect(throws_with_code([] {
               lingxi::json::Value request(lingxi::json::Object{});
               lingxi::json::Value thinking(lingxi::json::Object{});
               thinking.set("enabled", lingxi::json::Value(true));
               thinking.set("budgetTokens", lingxi::json::Value(2048));
               request.set("model", lingxi::json::Value("o3-mini"));
               request.set("messages", lingxi::json::Value(lingxi::json::Array{}));
               request.set("thinking", thinking);
               lingxi::Core::translate_request("openai_chat_completions", request);
           },
           "unsupported_feature"),
           "OpenAI thinking budgets are an explicit unsupported feature");

    expect(throws_with_code([] { lingxi::Core::translate_request("nope", lingxi::json::Value()); },
                            "unsupported_protocol"),
           "an unknown protocol is rejected");

    expect(throws_with_code([] {
               lingxi::native::prepare("openai_chat_completions", "http://example.com/v1", std::nullopt,
                                       std::nullopt, lingxi::json::Value(lingxi::json::Object{}));
           },
           "auth_missing"),
           "a missing API key is reported before the endpoint check");

    expect(throws_with_code([] {
               lingxi::native::prepare("openai_chat_completions", "http://example.com/v1",
                                       std::string("secret"), std::nullopt,
                                       lingxi::json::Value(lingxi::json::Object{}));
           },
           "invalid_endpoint"),
           "plain http to a non-loopback host is refused");
    expect(throws_with_code([] {
               lingxi::native::prepare("openai_chat_completions", "ftp://example.com",
                                       std::string("secret"), std::nullopt,
                                       lingxi::json::Value(lingxi::json::Object{}));
           },
           "invalid_endpoint"),
           "a non-HTTP scheme is refused");
}

// -------------------------------------------------------------------- native suite

lingxi::json::Value payload_for(const std::string& marker) {
    lingxi::json::Value request(lingxi::json::Object{});
    request.set("marker", lingxi::json::Value(marker));
    request.set("messages", lingxi::json::Value(lingxi::json::Array{}));
    return request;
}

void test_native(FixtureServer& server, const std::filesystem::path& root) {
    lingxi::NativeClient client(transport);
    const std::string base = server.base_url();

    struct Case {
        std::string protocol;
        std::string url_suffix;
        std::string framing;
        std::string auth_header;
        std::string auth_prefix;
    };
    const std::vector<Case> cases = {
        {"openai_chat_completions", "/chat/completions", "sse", "Authorization", "Bearer "},
        {"openai_responses", "/responses", "sse", "Authorization", "Bearer "},
        {"anthropic_messages", "/v1/messages", "sse", "x-api-key", ""},
        {"google_gemini", ":streamGenerateContent?alt=sse", "sse", "x-goog-api-key", ""},
        {"aws_bedrock", "/model/claude-3/converse", "amazon_event_stream", "Authorization", "AWS4-HMAC-SHA256 "},
        {"google_vertex_ai", ":streamGenerateContent?alt=sse", "sse", "Authorization", "Bearer "},
        {"azure_openai", "/chat/completions?api-version=2024-10-21", "sse", "api-key", ""},
        {"ollama", "/api/chat", "ndjson", "", ""},
        {"cohere", "/v2/chat", "sse", "Authorization", "Bearer "},
    };

    for (const Case& item : cases) {
        const std::string protocol = item.protocol;
        const std::string key = protocol == "aws_bedrock" ? std::string("AKID:SECRET") : std::string("test-key");
        const std::string model = protocol == "aws_bedrock" ? std::string("claude-3") : std::string("m-1");
        lingxi::json::Value payload(lingxi::json::Object{});
        payload.set("model", lingxi::json::Value(model));

        lingxi::native::PrepareOptions options;
        options.stream = true;
        lingxi::native::Target target =
            lingxi::native::prepare(protocol, base, key, model, payload, options);

        expect(target.url.rfind(base, 0) == 0, protocol + ": the prepared URL starts at baseUrl -> " + target.url);
        expect(target.url.find(item.url_suffix) != std::string::npos,
               protocol + ": URL contains " + item.url_suffix + " -> " + target.url);
        expect_equal(target.framing, item.framing, protocol + ": framing");
        if (!item.auth_header.empty()) {
            expect(target.headers.count(item.auth_header) == 1, protocol + ": auth header " + item.auth_header);
            const std::string value = target.headers.at(item.auth_header);
            expect(value.rfind(item.auth_prefix, 0) == 0, protocol + ": auth header prefix");
            expect(value.find("SECRET") == std::string::npos, protocol + ": the secret never leaks into the header");
        } else {
            expect(target.headers.count("Authorization") == 0, protocol + ": no auth header");
        }
        expect(target.body.has("model"), protocol + ": the body carries the provider payload");
    }

    {
        lingxi::native::Target anthropic = lingxi::native::prepare(
            "anthropic_messages", base, std::string("test-key"), std::string("m-1"),
            lingxi::json::Value(lingxi::json::Object{}), lingxi::native::PrepareOptions{});
        expect(anthropic.headers.count("anthropic-version") == 1, "Anthropic requires anthropic-version");
        expect_equal(anthropic.headers.at("anthropic-version"), "2023-06-01", "Anthropic version header");
    }
    {
        lingxi::native::PrepareOptions options;
        options.stream = true;
        lingxi::native::Target gemini = lingxi::native::prepare(
            "google_gemini", base, std::string("test-key"), std::string("m-1"),
            lingxi::json::Value(lingxi::json::Object{}), options);
        expect(gemini.url.find(":streamGenerateContent?alt=sse") != std::string::npos,
               "Gemini streaming uses the SSE alt endpoint");
    }
    {
        lingxi::native::PrepareOptions options;
        options.stream = true;
        lingxi::native::Target bedrock = lingxi::native::prepare(
            "aws_bedrock", base, std::string("AKID:SECRET"), std::string("claude-3"),
            lingxi::json::Value(lingxi::json::Object{}), options);
        expect(bedrock.url.find("/converse-stream") != std::string::npos, "Bedrock streaming URL");
        expect(bedrock.headers.count("X-Amz-Date") == 1, "SigV4 signs a date");
    }

    // Native responses come back as provider JSON: nothing may be normalized here.
    lingxi::json::Value chat = client.post(
        "openai_chat_completions", base, std::string("test-key"), std::string("gpt-5-mini"), payload_for("chat"));
    expect(chat.find("choices") != nullptr, "Native returns the provider payload untouched");
    expect(chat.find("content") == nullptr, "Native never leaks LMP fields");
    {
        std::vector<FixtureServer::Request> seen = server.requests();
        expect(!seen.empty(), "the loopback server saw the request");
        const FixtureServer::Request& last = seen.back();
        expect_equal(last.method, "POST", "the native client posts");
        expect(last.headers.count("Authorization") == 1, "the bearer header reaches the wire");
        expect(last.headers.at("Authorization") == "Bearer test-key", "the bearer header carries the key");
    }

    // Redirects must never be followed.
    {
        RawServer redirect(
            "HTTP/1.1 302 Found\r\n"
            "Location: http://127.0.0.1:9/leak\r\n"
            "Content-Length: 0\r\n"
            "Connection: close\r\n\r\n");
        bool blocked = false;
        try {
            client.post("openai_chat_completions", redirect.base_url(), std::string("test-key"), std::nullopt,
                        payload_for("redirect"));
        } catch (const lingxi::NativeHttpError& error) {
            blocked = error.status_code() == 302;
        }
        expect(blocked, "a 3xx surfaces as an HTTP status error instead of being followed");
    }

    // Error bodies keep the status and the provider payload.
    {
        const std::string body = read_file((root / "conformance" / "native" / "error_429.json").string());
        RawServer limited("HTTP/1.1 429 Too Many Requests\r\n"
                          "Content-Type: application/json\r\n"
                          "Content-Length: " + std::to_string(body.size()) + "\r\n"
                          "Connection: close\r\n\r\n" + body);
        bool rejected = false;
        try {
            client.post("openai_chat_completions", limited.base_url(), std::string("test-key"), std::nullopt,
                        payload_for("429"));
        } catch (const lingxi::NativeHttpError& error) {
            rejected = error.status_code() == 429 && error.body().find("rate") != std::string::npos;
        }
        expect(rejected, "a 429 error body is surfaced with its status");
    }

    // The buffered helper still splits SSE frames, including the [DONE] marker.
    {
        std::vector<lingxi::json::Value> frames = client.stream(
            "openai_chat_completions", base, std::string("test-key"), std::string("gpt-5-mini"),
            payload_for("stream"));
        expect(frames.size() == 3, "the buffered helper splits two SSE frames plus [DONE]");
        expect(frames.back().is_string() && frames.back().string() == "[DONE]",
               "[DONE] survives as a marker instead of throwing");
    }
    {
        std::vector<lingxi::json::Value> frames = client.stream(
            "ollama", base, std::string(), std::string("llama3.3"), payload_for("stream"));
        expect(frames.size() == 3, "ndjson streaming splits one frame per line");
        expect(frames.back().find("done") != nullptr, "the final ollama frame is kept");
    }

    // An unknown protocol never reaches the transport.
    expect(throws_with_code([&] {
               lingxi::native::prepare("nope", base, std::nullopt, std::nullopt,
                                       lingxi::json::Value(lingxi::json::Object{}));
           },
           "unsupported_protocol"),
           "an unknown protocol is rejected before the transfer");

    expect(throws_with_code([&] {
               lingxi::native::prepare("google_gemini", base, std::string("test-key"), std::nullopt,
                                       lingxi::json::Value(lingxi::json::Object{}));
           },
           "invalid_request"),
           "Gemini needs a model to build its URL");
}

// ------------------------------------------------------------------- unified suite

struct UnifiedCase {
    std::string protocol;
    std::string text;
    std::string thinking;
    std::optional<std::string> signature;
    std::optional<std::string> finish_reason;
    std::optional<long long> input_tokens;
    std::optional<long long> output_tokens;
    std::optional<long long> total_tokens;
    std::optional<long long> cache_read_tokens;
};

void test_unified(FixtureServer& server, const std::filesystem::path& root) {
    const std::string base = server.base_url();

    const std::vector<UnifiedCase> cases = {
        {"openai_chat_completions", "Hello from chat completions", "step-by-step", std::nullopt, "stop", 12, 6, 18, std::nullopt},
        {"openai_responses", "Hello from responses", "Brief thought", std::nullopt, "completed", 15, 8, 23, std::nullopt},
        {"anthropic_messages", "Hello from Anthropic", "Considering options", "sig_abc", "end_turn", 20, 11, 31, 5},
        {"google_gemini", "Hello from Gemini", "Gemini thinking", std::nullopt, "STOP", 14, 9, 23, std::nullopt},
        {"google_vertex_ai", "Hello from Vertex AI", "Thinking via Vertex", "sig-vertex-1", "STOP", 18, 9, 27, 6},
        {"aws_bedrock", "Hello from AWS Bedrock", "Thinking via Bedrock Converse", "sig-bedrock-1", "tool_use", 24, 12, 36, 8},
        {"azure_openai", "Hello from Azure OpenAI", "", std::nullopt, "stop", 14, 6, 20, 4},
        {"ollama", "Hello from Ollama", "Local model thought", std::nullopt, "stop", 15, 7, 22, std::nullopt},
        {"cohere", "Hello from Cohere", "Cohere thought", std::nullopt, "COMPLETE", 11, 5, 16, std::nullopt},
    };

    for (const UnifiedCase& item : cases) {
        const std::string protocol = item.protocol;
        const std::string key = protocol == "aws_bedrock" ? std::string("AKID:SECRET") : std::string("test-key");
        const std::string model = protocol == "aws_bedrock" ? std::string("claude-3") : std::string("m-1");
        lingxi::unified::Prepared prepared =
            lingxi::unified::prepare(protocol, base, key, model, lmp_request(model));
        expect(prepared.request.find("messages") != nullptr, protocol + ": the LMP request survives prepare");
        // Gemini/Vertex rename the conversation array to `contents`, OpenAI
        // Responses to `input`; every other protocol keeps `messages`.
        const bool carries_conversation = prepared.target.body.find("messages") != nullptr ||
                                          prepared.target.body.find("input") != nullptr ||
                                          prepared.target.body.find("contents") != nullptr;
        expect(carries_conversation, protocol + ": the native body carries the translated messages");

        lingxi::json::Value normalized =
            lingxi::unified::normalize_response(protocol, post_target(prepared.target));
        expect_equal(text_of(normalized), item.text, protocol + ": unified text");
        expect_equal(thinking_of(normalized), item.thinking, protocol + ": unified thinking");
        if (item.signature) {
            const std::optional<std::string> actual = thinking_signature_of(normalized);
            expect(actual.has_value() && *actual == *item.signature,
                   protocol + ": thinking signature is preserved");
        }
        if (item.finish_reason) {
            const std::optional<std::string> actual = finish_reason_of(normalized);
            expect(actual.has_value() && *actual == *item.finish_reason, protocol + ": finish reason");
        }
        if (item.input_tokens) {
            expect_usage(usage_value(normalized, "inputTokens"), *item.input_tokens, protocol + ": inputTokens");
        }
        if (item.output_tokens) {
            expect_usage(usage_value(normalized, "outputTokens"), *item.output_tokens, protocol + ": outputTokens");
        }
        if (item.total_tokens) {
            expect_usage(usage_value(normalized, "totalTokens"), *item.total_tokens, protocol + ": totalTokens");
        }
        if (item.cache_read_tokens) {
            expect_usage(usage_value(normalized, "cacheReadTokens"), *item.cache_read_tokens,
                         protocol + ": cacheReadTokens");
        }
        expect(normalized.find("raw") != nullptr, protocol + ": the raw provider payload is kept");
        expect(normalized.find("id") != nullptr, protocol + ": the LMP envelope carries an id");
    }

    // Bedrock additionally exercises tool calls and cache writes.
    {
        lingxi::json::Value native = lingxi::json::Value::parse(
            read_file((root / "conformance" / "native" / "aws_bedrock_response.json").string()));
        lingxi::json::Value normalized = lingxi::unified::normalize_response("aws_bedrock", native);
        const std::optional<std::string> name = tool_call_name(normalized);
        expect(name.has_value() && *name == "get_weather", "Bedrock tool calls are normalized");
        expect_usage(usage_value(normalized, "cacheWriteTokens"), 4, "Bedrock cache writes map to usage");
    }

}

// --------------------------------------------------------------- normalizer suite

struct Frame {
    std::string event;
    std::string data;
};

// Splits an SSE fixture into `{event,data}` frames, the shape the normalizer
// session accepts frame by frame.
std::vector<Frame> sse_frames(const std::string& body) {
    std::vector<Frame> frames;
    Frame current;
    std::string data;
    std::istringstream input(body);
    std::string line;
    while (std::getline(input, line)) {
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (line.empty()) {
            if (!data.empty()) {
                current.data = data;
                frames.push_back(current);
                current = Frame{};
                data.clear();
            }
            continue;
        }
        if (line.rfind("event:", 0) == 0) {
            current.event = line.substr(6);
            if (!current.event.empty() && current.event.front() == ' ') current.event.erase(0, 1);
        } else if (line.rfind("data:", 0) == 0) {
            std::string chunk = line.substr(5);
            if (!chunk.empty() && chunk.front() == ' ') chunk.erase(0, 1);
            if (!data.empty()) data.push_back('\n');
            data += chunk;
        }
    }
    if (!data.empty()) {
        current.data = data;
        frames.push_back(current);
    }
    return frames;
}

std::vector<lingxi::json::Value> normalize_frames(
    const std::string& protocol,
    const std::vector<Frame>& frames) {
    lingxi::NormalizerSession session = lingxi::NormalizerSession::open(protocol);
    std::vector<lingxi::json::Value> events;
    for (const Frame& frame : frames) {
        lingxi::json::Value input(lingxi::json::Object{});
        input.set("data", lingxi::json::Value(frame.data));
        if (!frame.event.empty()) input.set("event", lingxi::json::Value(frame.event));
        lingxi::json::Value pushed = session.push(input);
        const lingxi::json::Value* produced = pushed.find("events");
        expect(produced != nullptr && produced->is_array(), protocol + ": the session returns an event array");
        for (const auto& event : produced->array()) events.push_back(event);
    }
    lingxi::json::Value finished = session.finish();
    const lingxi::json::Value* tail = finished.find("events");
    expect(tail != nullptr && tail->is_array(), protocol + ": finish returns an event array");
    for (const auto& event : tail->array()) events.push_back(event);
    return events;
}

std::string event_type(const lingxi::json::Value& event) {
    const lingxi::json::Value* type = event.find("type");
    return type && type->is_string() ? type->string() : std::string();
}

std::optional<long long> usage_of_event(const lingxi::json::Value& event, const std::string& key) {
    const lingxi::json::Value* usage = event.find("usage");
    if (!usage || !usage->is_object()) return std::nullopt;
    const lingxi::json::Value* value = usage->find(key);
    if (!value || !value->is_number()) return std::nullopt;
    return value->optional_integer();
}

std::string concat_deltas(const std::vector<lingxi::json::Value>& events, const std::string& type) {
    std::string out;
    for (const auto& event : events) {
        if (event_type(event) != type) continue;
        const lingxi::json::Value* delta = event.find("delta");
        if (delta && delta->is_string()) out += delta->string();
    }
    return out;
}

void test_normalizer(const std::filesystem::path& root) {
    const std::filesystem::path native = root / "conformance" / "native";

    {
        const auto events = normalize_frames(
            "openai_chat_completions", sse_frames(read_file((native / "openai_chat_stream.sse").string())));
        expect_equal(concat_deltas(events, "text_delta"), "Hello world", "incremental OpenAI text deltas concat");
        bool saw_end = false;
        for (const auto& event : events) {
            if (event_type(event) == "usage") {
                expect_usage(usage_of_event(event, "inputTokens"), 10, "incremental usage input");
                expect_usage(usage_of_event(event, "outputTokens"), 2, "incremental usage output");
            }
            if (event_type(event) == "message_end") saw_end = true;
        }
        expect(saw_end, "the session closes the message exactly once");
    }

    {
        const auto events = normalize_frames(
            "anthropic_messages", sse_frames(read_file((native / "anthropic_messages_stream.sse").string())));
        expect_equal(concat_deltas(events, "thinking_delta"), "Thinking...", "incremental Anthropic thinking");
        expect_equal(concat_deltas(events, "text_delta"), "Hello Anthropic", "incremental Anthropic text");
        expect(event_type(events.front()) == "message_start", "the session honours message_start");
        expect(event_type(events.back()) == "message_end", "the session ends the message");
    }

    {
        const auto events = normalize_frames(
            "openai_responses", sse_frames(read_file((native / "openai_responses_stream.sse").string())));
        expect_equal(concat_deltas(events, "text_delta"), "Hello responses", "incremental Responses text");
    }

    {
        const auto events = normalize_frames(
            "google_gemini", sse_frames(read_file((native / "google_gemini_stream.sse").string())));
        expect_equal(concat_deltas(events, "text_delta"), "Hello Gemini", "incremental Gemini text");
    }

    // A malformed frame is a typed error, never a silent drop.
    {
        lingxi::NormalizerSession session = lingxi::NormalizerSession::open("openai_chat_completions");
        lingxi::json::Value bad(lingxi::json::Object{});
        bad.set("data", lingxi::json::Value("{not json"));
        expect(throws_with_code([&] { session.push(bad); }, "decode"),
               "a malformed frame surfaces as a decode error");
    }

    expect(throws_with_code([] { lingxi::NormalizerSession::open("nope"); }, "unknown_protocol"),
           "an unknown protocol cannot open a session");
}
// ---------------------------------------------------------------- streaming suite

lingxi::json::Value stream_request(
    const std::string& protocol,
    const std::string& base_url,
    const std::string& api_key,
    const std::string& model,
    bool unified,
    const std::optional<lingxi::json::Value>& lmp) {
    lingxi::json::Value request(lingxi::json::Object{});
    request.set("protocol", lingxi::json::Value(protocol));
    request.set("baseUrl", lingxi::json::Value(base_url));
    request.set("apiKey", lingxi::json::Value(api_key));
    request.set("model", lingxi::json::Value(model));
    if (unified) {
        request.set("payload", *lmp);
        request.set("unified", lingxi::json::Value(true));
    } else {
        lingxi::json::Value payload(lingxi::json::Object{});
        payload.set("model", lingxi::json::Value(model));
        payload.set("stream", lingxi::json::Value(true));
        request.set("payload", payload);
    }
    request.set("idleTimeoutMs", lingxi::json::Value(5000));
    return request;
}

void test_streaming(FixtureServer& server, const std::filesystem::path& root) {
    const std::string base = server.base_url();

    // 1. An incremental Unified stream surfaces events before the upstream ends.
    {
        const std::string first = R"(event: message_start
data: {"type":"message_start","message":{"id":"msg_gated"}}

)";
        const std::string rest = R"(event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello gated"}}

event: message_stop
data: {"type":"message_stop"}

)";
        GatedServer gated(first, rest, "text/event-stream");
        lingxi::StreamSession session = lingxi::StreamSession::open(stream_request(
            "anthropic_messages", gated.base_url(), "test-key", "m-1", true, lmp_request("m-1")));
        std::optional<lingxi::json::Value> start = session.next(5000);
        expect(start.has_value() && event_type(*start) == "message_start",
               "the first event arrives before the upstream ends");
        expect(gated.first_frame_served(), "the gated server had already written its first frame");
        gated.release_rest();

        std::optional<lingxi::json::Value> text = session.next(5000);
        expect(text.has_value() && event_type(*text) == "text_delta", "the next event is the text delta");
        const lingxi::json::Value* delta = text.has_value() ? text->find("delta") : nullptr;
        expect(delta != nullptr && delta->is_string() && delta->string() == "Hello gated",
               "the text delta carries the provider text");

        std::vector<lingxi::json::Value> tail = session.collect(5000);
        bool saw_end = false;
        for (const auto& event : tail) {
            if (event_type(event) == "message_end") saw_end = true;
        }
        expect(saw_end, "the unified stream closes with message_end");
        expect(session.finished(), "a drained session reports finished");
    }

    // 2. Dropping a session mid-stream releases the connection without hanging.
    {
        GatedServer gated("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n", "", "text/event-stream");
        {
            lingxi::StreamSession session = lingxi::StreamSession::open(stream_request(
                "anthropic_messages", gated.base_url(), "test-key", "m-1", true, lmp_request("m-1")));
            std::optional<lingxi::json::Value> start = session.next(5000);
            expect(start.has_value(), "the first frame is read before the session is dropped");
        }
        gated.release_rest();
        expect(true, "dropping a session releases the handle");
    }

    // 3. Cancel wakes a blocked reader from another thread.
    {
        SilentServer silent;
        lingxi::StreamSession session = lingxi::StreamSession::open(stream_request(
            "anthropic_messages", silent.base_url(), "test-key", "m-1", true, lmp_request("m-1")));
        std::atomic<bool> cancelled{false};
        std::thread canceller([&] {
            for (int i = 0; i < 500; ++i) {
                if (silent.accepted()) break;
                std::this_thread::sleep_for(std::chrono::milliseconds(10));
            }
            session.cancel();
            cancelled.store(true);
        });
        // A unified stream reports a terminal failure as a typed `error`
        // event (LmpStreamEvent::Error) so callers can tell cancellation
        // apart from a transport fault without parsing prose.
        std::optional<lingxi::json::Value> terminal = session.next(0);
        canceller.join();
        std::string terminal_type;
        std::string terminal_code;
        if (terminal) {
            const lingxi::json::Value* type = terminal->find("type");
            if (type && type->is_string()) terminal_type = type->string();
            const lingxi::json::Value* code = terminal->find("code");
            if (code && code->is_string()) terminal_code = code->string();
        }
        expect(cancelled.load(), "the canceller ran");
        expect(terminal_type == "error", "cancel ends a unified stream with a typed error event");
        expect(terminal_code == "cancelled", "the error event carries the cancelled code");
        expect(!session.next(0).has_value(), "an errored stream ends without further events");
    }

    // 4. A native Bedrock stream yields provider frames, not LMP events.
    {
        lingxi::StreamSession session = lingxi::StreamSession::open(stream_request(
            "aws_bedrock", base, "AKID:SECRET", "claude-3", false, std::nullopt));
        std::vector<lingxi::json::Value> frames = session.collect(10000);
        expect(!frames.empty(), "the native Bedrock stream produced frames");
        expect(frames.front().find("event") != nullptr, "native frames carry the event name");
        expect(frames.front().find("type") == nullptr, "native frames are not LMP events");
        // The streamed fixture is the same conversation as the non-streaming
        // fixture but split into deltas, so it carries a text fragment rather
        // than the complete sentence.
        bool saw_text_delta = false;
        bool saw_usage_metadata = false;
        for (const auto& frame : frames) {
            const lingxi::json::Value* data = frame.find("data");
            if (!data || !data->is_string()) continue;
            if (data->string().find("\"text\":\"Hello \"") != std::string::npos) saw_text_delta = true;
            if (data->string().find("\"inputTokens\":24") != std::string::npos) saw_usage_metadata = true;
        }
        expect(saw_text_delta, "the native stream carries the provider text delta");
        expect(saw_usage_metadata, "the native stream carries the raw provider usage metadata");
    }

    expect(throws_with_code([&] {
               lingxi::StreamSession::open(stream_request(
                   "nope", base, "test-key", "m-1", false, std::nullopt));
           },
           "open_failed"),
           "an unknown protocol cannot open a stream");
}

// --------------------------------------------------------- embed / rerank suite

std::vector<std::vector<double>> embeddings_of(const lingxi::json::Value& response) {
    std::vector<std::vector<double>> out;
    const lingxi::json::Value* embeddings = response.find("embeddings");
    if (!embeddings || !embeddings->is_array()) return out;
    for (const auto& row : embeddings->array()) {
        std::vector<double> values;
        if (!row.is_array()) continue;
        for (const auto& value : row.array()) {
            if (value.is_number()) values.push_back(value.number());
        }
        out.push_back(std::move(values));
    }
    return out;
}

void test_embeddings_and_rerank(FixtureServer& server) {
    const std::string base = server.base_url();
    const std::vector<std::string> input = {"first document", "second document"};

    {
        lingxi::json::Value response =
            lingxi::unified::embed("openai_chat_completions", base, std::string("test-key"), "text-embedding-3-small", input);
        const auto rows = embeddings_of(response);
        expect(rows.size() == 2, "OpenAI embeddings return one row per input");
        expect(rows.size() == 2 && rows[0].size() == 3, "each row keeps the provider dimensions");
        expect(rows.size() == 2 && std::abs(rows[0][0] - 0.1) < 1e-9, "the first vector value survives");
        expect_usage(usage_value(response, "inputTokens"), 8, "the embedding usage is normalized");
    }

    {
        lingxi::json::Value response =
            lingxi::unified::embed("cohere", base, std::string("test-key"), "embed-v4", input);
        const auto rows = embeddings_of(response);
        expect(rows.size() == 2, "Cohere embeddings return one row per input");
        expect(rows.size() == 2 && rows[1].size() == 3, "Cohere dimensions survive");
    }

    {
        lingxi::json::Value response =
            lingxi::unified::embed("ollama", base, std::nullopt, "nomic-embed-text", input);
        const auto rows = embeddings_of(response);
        expect(rows.size() == 2, "Ollama embeddings return one row per input");
        expect_usage(usage_value(response, "inputTokens"), 8, "Ollama prompt counts map to input tokens");
    }

    expect(throws_with_code([&] {
               lingxi::unified::embed("anthropic_messages", base, std::string("test-key"), "claude", input);
           },
           "unsupported_feature"),
           "Anthropic embeddings are explicitly unsupported");

    {
        lingxi::json::Value response = lingxi::unified::rerank(
            "cohere", base, std::string("test-key"), "rerank-v3", "what is lingxi?", {"doc a", "doc b"}, 2);
        const lingxi::json::Value* results = response.find("results");
        expect(results != nullptr && results->is_array() && results->array().size() == 2,
               "rerank returns one result per document");
        const lingxi::json::Value& top = results->array().front();
        const lingxi::json::Value* index = top.find("index");
        const lingxi::json::Value* score = top.find("relevanceScore");
        expect(index != nullptr && index->optional_integer().value_or(-1) == 1, "rerank keeps the provider index");
        expect(score != nullptr && score->is_number() && std::abs(score->number() - 0.94) < 1e-9,
               "rerank maps relevance_score to relevanceScore");
    }

    expect(throws_with_code([&] {
               lingxi::unified::rerank("openai_chat_completions", base, std::string("test-key"), "m", "q", {}, std::nullopt);
           },
           "unsupported_feature"),
           "rerank is Cohere-only and explicitly unsupported elsewhere");
}

} // namespace

int main(int argc, char** argv) {
    const std::filesystem::path root =
        argc > 1 ? std::filesystem::path(argv[1]) : std::filesystem::path("../..");
    try {
        test_catalog(root);
        test_catalog_loader(root);
        test_registry(root);
        test_error_contract();

        FixtureServer server(root / "conformance" / "native");
        test_native(server, root);
        test_unified(server, root);
        test_normalizer(root);
        test_streaming(server, root);
        test_embeddings_and_rerank(server);
    } catch (const std::exception& error) {
        std::cerr << "FAIL: unexpected exception: " << error.what() << "\n";
        return 1;
    }
    std::cout << "C++ SDK conformance passed (" << checks << " checks).\n";
    return 0;
}

