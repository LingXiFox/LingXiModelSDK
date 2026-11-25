#pragma once

#include "lingxi/core.hpp"
#include "lingxi/json.hpp"

#include <string>
#include <utility>

namespace lingxi {

// Incremental LMP normalization for hosts that own the HTTP transport:
// the session holds protocol state (tool-call argument accumulation, thinking
// signatures, usage merging) and performs no I/O, so it is safe to call from
// the host event loop. Feed raw SSE/NDJSON frames as they arrive.
class NormalizerSession {
public:
    static NormalizerSession open(const std::string& protocol) {
        LingxiNormalizer* handle = lingxi_lmp_normalizer_open(protocol.c_str());
        if (handle == nullptr) {
            std::string message;
            if (char* raw = lingxi_normalizer_open_error()) {
                message.assign(raw);
                lingxi_string_free(raw);
            }
            throw CoreError("lingxi_lmp_normalizer_open", "unknown_protocol", message);
        }
        return NormalizerSession(handle, protocol);
    }

    NormalizerSession(NormalizerSession&& other) noexcept
        : handle_(std::exchange(other.handle_, nullptr)), protocol_(std::move(other.protocol_)) {}

    NormalizerSession& operator=(NormalizerSession&& other) noexcept {
        if (this != &other) {
            close();
            handle_ = std::exchange(other.handle_, nullptr);
            protocol_ = std::move(other.protocol_);
        }
        return *this;
    }

    NormalizerSession(const NormalizerSession&) = delete;
    NormalizerSession& operator=(const NormalizerSession&) = delete;

    ~NormalizerSession() { close(); }

    // One frame: {"event":..,"data":..,"id":..} or the bare provider frame.
    // Returns {"events":[...],"done":bool}.
    json::Value push(const json::Value& frame) {
        require_handle();
        std::string body = frame.dump();
        return detail::require_ok(
            "lingxi_lmp_normalizer_push",
            lingxi_lmp_normalizer_push(handle_, body.c_str()));
    }

    // Emits trailing state such as the final message_end.
    json::Value finish() {
        require_handle();
        return detail::require_ok(
            "lingxi_lmp_normalizer_finish", lingxi_lmp_normalizer_finish(handle_));
    }

    void close() noexcept {
        if (handle_ != nullptr) {
            lingxi_lmp_normalizer_close(handle_);
            handle_ = nullptr;
        }
    }

    const std::string& protocol() const { return protocol_; }
    bool valid() const { return handle_ != nullptr; }

private:
    NormalizerSession(LingxiNormalizer* handle, std::string protocol)
        : handle_(handle), protocol_(std::move(protocol)) {}

    void require_handle() const {
        if (handle_ == nullptr) {
            throw CoreError("lingxi_lmp_normalizer_push", "closed", "the normalizer session is closed");
        }
    }

    LingxiNormalizer* handle_ = nullptr;
    std::string protocol_;
};

} // namespace lingxi
