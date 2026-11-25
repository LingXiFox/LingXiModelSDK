#pragma once

#include "lingxi/core.hpp"
#include "lingxi/json.hpp"

#include <cstdint>
#include <optional>
#include <vector>
#include <string>
#include <utility>

namespace lingxi {

// One open upstream response. The core owns the socket and its runtime; each
// next() call reads exactly the bytes needed for the next frame, so a slow
// consumer naturally applies backpressure instead of buffering the stream.
//
// next() blocks: never call it from a thread that must keep serving events.
// cancel() may be called from any other thread while next() is blocked.
class StreamSession {
public:
    // request: protocol, baseUrl, apiKey?, model?, payload, headers, timeoutMs,
    // idleTimeoutMs, unified. Returns once response headers arrive.
    static StreamSession open(const json::Value& request) {
        std::string body = request.dump();
        LingxiStream* handle = lingxi_stream_open(body.c_str());
        if (handle == nullptr) {
            std::string message;
            std::string code = "open_failed";
            if (char* raw = lingxi_stream_open_error()) {
                std::string text(raw);
                lingxi_string_free(raw);
                message = text;
                try {
                    json::Value envelope = json::Value::parse(text);
                    if (const json::Value* found = envelope.find("code"); found && found->is_string()) {
                        code = found->string();
                    }
                    if (const json::Value* found = envelope.find("error"); found && found->is_string()) {
                        message = found->string();
                    }
                } catch (const std::exception&) {
                    // Keep the raw text as the message.
                }
            }
            throw CoreError("lingxi_stream_open", code, message);
        }
        return StreamSession(handle);
    }

    StreamSession(StreamSession&& other) noexcept
        : handle_(std::exchange(other.handle_, nullptr)), finished_(other.finished_) {}

    StreamSession& operator=(StreamSession&& other) noexcept {
        if (this != &other) {
            close();
            handle_ = std::exchange(other.handle_, nullptr);
            finished_ = other.finished_;
        }
        return *this;
    }

    StreamSession(const StreamSession&) = delete;
    StreamSession& operator=(const StreamSession&) = delete;

    ~StreamSession() { close(); }

    // Next event payload: the provider frame for a native stream, the LMP event
    // for a unified stream. nullopt means the upstream ended cleanly.
    std::optional<json::Value> next(std::uint64_t timeout_ms = 0) {
        if (handle_ == nullptr) {
            throw CoreError("lingxi_stream_next", "closed", "the stream session is closed");
        }
        if (finished_) return std::nullopt;

        detail::Envelope envelope = detail::parse_envelope(
            "lingxi_stream_next", lingxi_stream_next(handle_, timeout_ms));
        if (!envelope.ok) {
            finished_ = true;
            throw CoreError(
                "lingxi_stream_next",
                envelope.code.empty() ? std::string("stream_error") : envelope.code,
                envelope.error);
        }
        if (const json::Value* done = envelope.value.find("done"); done && done->is_bool() && done->boolean()) {
            finished_ = true;
            return std::nullopt;
        }
        const json::Value* event = envelope.value.find("event");
        if (event == nullptr) {
            finished_ = true;
            return std::nullopt;
        }
        return *event;
    }

    // Drains every remaining event into a vector, then the session is done.
    std::vector<json::Value> collect(std::uint64_t timeout_ms = 0) {
        std::vector<json::Value> events;
        while (std::optional<json::Value> event = next(timeout_ms)) {
            events.push_back(std::move(*event));
        }
        return events;
    }

    void cancel() noexcept {
        if (handle_ != nullptr) lingxi_stream_cancel(handle_);
    }

    void close() noexcept {
        if (handle_ != nullptr) {
            lingxi_stream_close(handle_);
            handle_ = nullptr;
        }
        finished_ = true;
    }

    bool valid() const { return handle_ != nullptr; }
    bool finished() const { return finished_; }

private:
    explicit StreamSession(LingxiStream* handle) : handle_(handle) {}

    LingxiStream* handle_ = nullptr;
    bool finished_ = false;
};

} // namespace lingxi
