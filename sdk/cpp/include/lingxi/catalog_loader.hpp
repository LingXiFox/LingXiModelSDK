#pragma once

#include "lingxi/catalog.hpp"
#include "lingxi/json.hpp"

#include <chrono>
#include <filesystem>
#include <fstream>
#include <functional>
#include <map>
#include <mutex>
#include <optional>
#include <sstream>
#include <string>

namespace lingxi::catalog {

struct HttpReply {
    int status_code = 0;
    std::string body;
    std::optional<std::string> etag;
};

// The loader never owns a socket: give it whatever transport the host already
// has (libcurl, Beast, an app-internal client) and it handles freshness,
// conditional requests, the disk cache and stale fallback.
using Transport = std::function<HttpReply(
    const std::string& url,
    const std::map<std::string, std::string>& headers,
    std::chrono::milliseconds timeout)>;

struct LoaderConfig {
    static constexpr const char* kDefaultEndpoint = "https://models.lingxifox.cn/models.json";

    std::string endpoint = kDefaultEndpoint;
    std::chrono::seconds max_age = std::chrono::minutes(30);
    std::optional<std::filesystem::path> cache_directory;
    std::chrono::milliseconds request_timeout = std::chrono::seconds(10);
    std::string user_agent = "LingXiModelSDK-Cpp/2.0";
};

struct CachedCatalog {
    ModelCatalog catalog;
    std::optional<std::string> etag;
    std::chrono::system_clock::time_point fetched_at;

    bool is_fresh(std::chrono::seconds max_age) const {
        if (max_age.count() <= 0) return false;
        auto age = std::chrono::duration_cast<std::chrono::seconds>(
            std::chrono::system_clock::now() - fetched_at);
        return age < max_age;
    }
};

namespace detail {

inline std::string read_file(const std::filesystem::path& path) {
    std::ifstream input(path, std::ios::binary);
    if (!input.good()) throw std::runtime_error("cannot open " + path.string());
    std::ostringstream buffer;
    buffer << input.rdbuf();
    return buffer.str();
}

inline void write_file(const std::filesystem::path& path, const std::string& text) {
    std::filesystem::create_directories(path.parent_path());
    std::filesystem::path temporary = path;
    temporary += ".tmp";
    {
        std::ofstream output(temporary, std::ios::binary | std::ios::trunc);
        if (!output.good()) throw std::runtime_error("cannot write " + temporary.string());
        output << text;
    }
    std::filesystem::rename(temporary, path);
}

inline std::string timestamp_to_iso(std::chrono::system_clock::time_point at) {
    std::time_t seconds = std::chrono::system_clock::to_time_t(at);
    std::tm utc{};
    gmtime_r(&seconds, &utc);
    char buffer[32];
    std::strftime(buffer, sizeof(buffer), "%Y-%m-%dT%H:%M:%SZ", &utc);
    return std::string(buffer);
}

} // namespace detail

class CatalogLoader {
public:
    CatalogLoader(LoaderConfig config, Transport transport)
        : config_(std::move(config)), transport_(std::move(transport)) {}

    const LoaderConfig& config() const { return config_; }

    std::optional<CachedCatalog> cached() {
        std::lock_guard<std::mutex> guard(mutex_);
        return load_cache_locked();
    }

    ModelCatalog load(bool force_refresh = false) {
        std::optional<CachedCatalog> cached_catalog = cached();
        if (!force_refresh && cached_catalog && cached_catalog->is_fresh(config_.max_age)) {
            return cached_catalog->catalog;
        }

        std::map<std::string, std::string> headers;
        headers["Accept"] = "application/json";
        headers["User-Agent"] = config_.user_agent;
        if (cached_catalog && cached_catalog->etag && !cached_catalog->etag->empty()) {
            headers["If-None-Match"] = *cached_catalog->etag;
        }

        HttpReply reply;
        try {
            reply = transport_(config_.endpoint, headers, config_.request_timeout);
        } catch (const std::exception& error) {
            if (cached_catalog) return cached_catalog->catalog;
            throw CatalogError(CatalogError::Kind::Transport, error.what());
        }

        if (reply.status_code == 304 && cached_catalog) {
            CachedCatalog refreshed = *cached_catalog;
            refreshed.fetched_at = std::chrono::system_clock::now();
            if (reply.etag) refreshed.etag = reply.etag;
            std::lock_guard<std::mutex> guard(mutex_);
            memory_ = refreshed;
            persist_meta_locked(refreshed);
            return refreshed.catalog;
        }
        if (reply.status_code < 200 || reply.status_code > 299) {
            if (cached_catalog) return cached_catalog->catalog;
            throw CatalogError(
                CatalogError::Kind::UnexpectedStatus,
                "Unexpected HTTP status: " + std::to_string(reply.status_code),
                "",
                reply.status_code);
        }

        ModelCatalog catalog = ModelCatalog::decode(reply.body);
        CachedCatalog fresh{std::move(catalog), reply.etag, std::chrono::system_clock::now()};
        {
            std::lock_guard<std::mutex> guard(mutex_);
            memory_ = fresh;
            persist_document_locked(reply.body);
            persist_meta_locked(fresh);
        }
        return fresh.catalog;
    }

private:
    std::optional<std::filesystem::path> document_path() const {
        if (!config_.cache_directory) return std::nullopt;
        return *config_.cache_directory / "models.json";
    }

    std::optional<std::filesystem::path> meta_path() const {
        if (!config_.cache_directory) return std::nullopt;
        return *config_.cache_directory / "models.json.meta.json";
    }

    std::optional<CachedCatalog> load_cache_locked() {
        if (memory_) return memory_;
        std::optional<std::filesystem::path> path = document_path();
        if (!path || !std::filesystem::exists(*path)) return std::nullopt;
        try {
            std::string raw = detail::read_file(*path);
            ModelCatalog catalog = ModelCatalog::decode(raw);
            CachedCatalog entry{std::move(catalog), std::nullopt, std::chrono::system_clock::time_point{}};
            if (std::optional<std::filesystem::path> meta = meta_path(); meta && std::filesystem::exists(*meta)) {
                lingxi::json::Value parsed = lingxi::json::Value::parse(detail::read_file(*meta));
                if (const auto* etag = parsed.find("etag"); etag && etag->is_string()) {
                    entry.etag = etag->string();
                }
            }
            memory_ = entry;
            return entry;
        } catch (const std::exception&) {
            return std::nullopt;
        }
    }

    void persist_document_locked(const std::string& document) {
        if (std::optional<std::filesystem::path> path = document_path()) {
            try {
                detail::write_file(*path, document);
            } catch (const std::exception&) {
                // A read-only cache directory must not fail a successful load.
            }
        }
    }

    void persist_meta_locked(const CachedCatalog& entry) {
        std::optional<std::filesystem::path> path = meta_path();
        if (!path) return;
        lingxi::json::Value meta(lingxi::json::Object{});
        if (entry.etag) meta.set("etag", lingxi::json::Value(*entry.etag));
        meta.set("fetchedAt", lingxi::json::Value(detail::timestamp_to_iso(entry.fetched_at)));
        try {
            detail::write_file(*path, meta.dump());
        } catch (const std::exception&) {
            // Same rationale as the document cache.
        }
    }

    LoaderConfig config_;
    Transport transport_;
    std::mutex mutex_;
    std::optional<CachedCatalog> memory_;
};

} // namespace lingxi::catalog
