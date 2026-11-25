#pragma once

#include "lingxi/core.hpp"
#include "lingxi/json.hpp"
#include "lingxi/native.hpp"

#include <chrono>
#include <functional>
#include <map>
#include <optional>
#include <stdexcept>
#include <string>
#include <vector>

namespace lingxi {

struct HttpReply {
    int status_code = 0;
    std::map<std::string, std::string> headers;
    std::string body;
};

// The SDK deliberately ships no HTTP client: inject whatever the host already
// uses (libcurl, Beast, an internal stack). The transport receives the final
// URL, auth headers and serialized body the Rust core prepared.
using Transport = std::function<HttpReply(
    const std::string& url,
    const std::map<std::string, std::string>& headers,
    const std::string& body,
    std::chrono::milliseconds timeout)>;

class NativeHttpError : public std::runtime_error {
public:
    NativeHttpError(int status_code, std::string body)
        : std::runtime_error("HTTP " + std::to_string(status_code) + ": " + body),
          status_code_(status_code),
          body_(std::move(body)) {}

    int status_code() const { return status_code_; }
    const std::string& body() const { return body_; }

private:
    int status_code_;
    std::string body_;
};

// Native Provider client: the core resolves URL/auth/SigV4/body, the injected
// transport performs the exchange, and the parsed provider JSON comes back
// untouched (Native must never be normalized silently).
class NativeClient {
public:
    explicit NativeClient(Transport transport) : transport_(std::move(transport)) {}

    const Transport& transport() const { return transport_; }

    json::Value post(
        const std::string& protocol,
        const std::string& base_url,
        const std::optional<std::string>& api_key,
        const std::optional<std::string>& model,
        const json::Value& payload,
        const native::PrepareOptions& options = {}) {
        native::Target target =
            native::prepare(protocol, base_url, api_key, model, payload, options);
        std::chrono::milliseconds timeout(options.timeout_ms.value_or(60000));
        HttpReply reply = transport_(target.url, target.headers, target.body.dump(), timeout);
        if (reply.status_code < 200 || reply.status_code > 299) {
            throw NativeHttpError(reply.status_code, reply.body);
        }
        return json::Value::parse(reply.body);
    }

    // Raw SSE/NDJSON frames for a native streaming request (buffered helper).
    std::vector<json::Value> stream(
        const std::string& protocol,
        const std::string& base_url,
        const std::optional<std::string>& api_key,
        const std::optional<std::string>& model,
        const json::Value& payload,
        const native::PrepareOptions& options = {}) {
        native::PrepareOptions streamed = options;
        streamed.stream = true;
        native::Target target =
            native::prepare(protocol, base_url, api_key, model, payload, streamed);
        std::chrono::milliseconds timeout(options.timeout_ms.value_or(60000));
        HttpReply reply = transport_(target.url, target.headers, target.body.dump(), timeout);
        if (reply.status_code < 200 || reply.status_code > 299) {
            throw NativeHttpError(reply.status_code, reply.body);
        }
        return split_frames(reply.body, target.framing);
    }

    // Splits a buffered SSE/NDJSON body into JSON frames. Amazon Event Stream
    // is binary, so it is handled by the incremental StreamSession instead.
    static std::vector<json::Value> split_frames(const std::string& body, const std::string& framing) {
        std::vector<json::Value> frames;
        if (framing == "ndjson" || framing == "amazon_event_stream") {
            std::size_t start = 0;
            while (start < body.size()) {
                std::size_t end = body.find('\n', start);
                if (end == std::string::npos) end = body.size();
                std::string line = body.substr(start, end - start);
                start = end + 1;
                trim(line);
                if (line.empty()) continue;
                frames.push_back(parse_frame(line));
            }
            return frames;
        }
        std::size_t start = 0;
        std::string data;
        while (start < body.size()) {
            std::size_t end = body.find('\n', start);
            if (end == std::string::npos) end = body.size();
            std::string line = body.substr(start, end - start);
            start = end + 1;
            if (!line.empty() && line.back() == '\r') line.pop_back();
            if (line.empty()) {
                if (!data.empty()) {
                    frames.push_back(parse_frame(data));
                    data.clear();
                }
                continue;
            }
            if (line.rfind("data:", 0) == 0) {
                std::string chunk = line.substr(5);
                if (!chunk.empty() && chunk.front() == ' ') chunk.erase(0, 1);
                if (!data.empty()) data.push_back('\n');
                data += chunk;
            }
        }
        if (!data.empty()) frames.push_back(parse_frame(data));
        return frames;
    }
private:
    // `[DONE]` is an SSE control marker, not JSON, so it survives as a string
    // marker instead of being dropped or throwing.
    static json::Value parse_frame(std::string text) {
        trim(text);
        if (text == "[DONE]" || text == "[done]") return json::Value("[DONE]");
        return json::Value::parse(text);
    }


    static void trim(std::string& text) {
        std::size_t first = text.find_first_not_of(" \t\r\n");
        std::size_t last = text.find_last_not_of(" \t\r\n");
        if (first == std::string::npos) {
            text.clear();
            return;
        }
        text = text.substr(first, last - first + 1);
    }

    Transport transport_;
};

} // namespace lingxi
