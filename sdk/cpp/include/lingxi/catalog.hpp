#pragma once

#include <algorithm>
#include <optional>
#include <stdexcept>
#include <string>
#include <vector>

namespace lingxi::catalog {

struct ModelSummary {
    std::string id;
    std::string provider_id;
    std::string name;
};

// Lightweight zero-FFI catalog parser for C++ consumers who only need metadata.
class ModelCatalog {
public:
    explicit ModelCatalog(std::string raw_json) : raw_(std::move(raw_json)) {
        if (raw_.find("\"schemaVersion\"") == std::string::npos ||
            raw_.find("\"2.0\"") == std::string::npos) {
            throw std::runtime_error("Unsupported or invalid catalog JSON (expected schemaVersion 2.0)");
        }
    }

    const std::string& raw() const noexcept { return raw_; }

    bool contains_provider(const std::string& provider_id) const {
        return raw_.find("\"" + provider_id + "\"") != std::string::npos;
    }

private:
    std::string raw_;
};

} // namespace lingxi::catalog
