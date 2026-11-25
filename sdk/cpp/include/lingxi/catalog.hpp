#pragma once

#include "lingxi/json.hpp"

#include <algorithm>
#include <chrono>
#include <optional>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace lingxi::catalog {

using json::Value;

enum class ModelStatus { Active, Preview, Deprecated, Retired, Unknown };

inline ModelStatus model_status_from_raw(const std::optional<std::string>& raw) {
    if (!raw) return ModelStatus::Unknown;
    if (*raw == "deprecated") return ModelStatus::Deprecated;
    if (*raw == "retired") return ModelStatus::Retired;
    if (*raw == "beta" || *raw == "preview") return ModelStatus::Preview;
    if (*raw == "active") return ModelStatus::Active;
    return ModelStatus::Unknown;
}

inline bool is_selectable(ModelStatus status) {
    return status != ModelStatus::Deprecated && status != ModelStatus::Retired;
}

struct ModelLimits {
    std::optional<long long> context;
    std::optional<long long> input;
    std::optional<long long> output;
};

struct ModelCapabilities {
    bool reasoning = false;
    std::vector<std::string> reasoning_efforts;
    bool tool_calling = false;
    bool structured_output = false;
    bool attachments = false;
    bool vision = false;
    bool audio_input = false;
    bool audio_output = false;
    std::vector<std::string> input_modalities;
    std::vector<std::string> output_modalities;
    bool open_weights = false;
    std::optional<bool> supports_temperature;
};

struct ModelPricing {
    std::optional<double> input;
    std::optional<double> output;
    std::optional<double> cache_read;
    std::optional<double> cache_write;
    std::optional<double> reasoning;
    std::optional<double> input_audio;
    std::optional<double> output_audio;

    bool is_unknown() const {
        return !input && !output && !cache_read && !cache_write && !reasoning && !input_audio && !output_audio;
    }
};

struct CatalogModel {
    std::string id;
    std::string provider_id;
    std::string name;
    std::optional<std::string> family;
    std::optional<std::string> overview;
    std::optional<std::string> release_date;
    std::optional<std::string> last_updated;
    std::optional<std::string> knowledge_cutoff;
    std::optional<std::string> canonical_model_id;
    ModelStatus status = ModelStatus::Unknown;
    ModelLimits limits;
    ModelCapabilities capabilities;
    ModelPricing pricing;
    Value fields;

    std::string qualified_id() const { return provider_id + "/" + id; }
    std::optional<long long> context_window() const { return limits.context; }
    std::optional<long long> max_output_tokens() const { return limits.output; }
};

struct CatalogProvider {
    std::string id;
    std::string name;
    std::optional<std::string> base_url;
    std::optional<std::string> documentation_url;
    std::vector<std::string> environment_variable_names;
    std::vector<CatalogModel> models;
    Value fields;

    std::size_t model_count() const { return models.size(); }
};

struct CatalogRevision {
    std::string schema_version;
    std::string catalog_revision;
    std::optional<std::string> catalog_hash;
    std::optional<std::string> source;
    std::optional<std::string> source_url;
    std::optional<std::string> source_hash;
    std::optional<std::string> generated_at;
    std::optional<std::string> source_fetched_at;
    int total_providers = 0;
    int total_models = 0;
};

struct ModelFilter {
    std::optional<std::string> provider_id;
    std::optional<std::string> query;
    std::optional<bool> reasoning;
    std::optional<bool> tool_calling;
    std::optional<bool> structured_output;
    std::optional<bool> vision;
    std::optional<bool> attachments;
    std::optional<bool> open_weights;
    std::optional<long long> minimum_context_window;
    std::optional<long long> minimum_max_output_tokens;
    bool include_deprecated = false;
    std::vector<std::string> featured_provider_ids;
};

class CatalogError : public std::runtime_error {
public:
    enum class Kind { InvalidCatalog, UnsupportedSchemaVersion, Transport, UnexpectedStatus };

    CatalogError(
        Kind kind,
        const std::string& message,
        std::string schema_version = {},
        int status_code = 0)
        : std::runtime_error(message),
          kind_(kind),
          schema_version_(std::move(schema_version)),
          status_code_(status_code) {}

    Kind kind() const { return kind_; }
    const std::string& schema_version() const { return schema_version_; }
    int status_code() const { return status_code_; }

private:
    Kind kind_;
    std::string schema_version_;
    int status_code_;
};

// Natural ordering: digit runs compare numerically, everything else compares
// case-insensitively. Matches the conformance ordering used by every SDK.
inline std::string ascii_lower(const std::string& text) {
    std::string out = text;
    for (char& ch : out) {
        if (ch >= 'A' && ch <= 'Z') ch = char(ch - 'A' + 'a');
    }
    return out;
}

struct SortToken {
    bool is_number = false;
    std::string raw;
    std::string trimmed;
};

inline std::vector<SortToken> tokenize(const std::string& text) {
    std::vector<SortToken> tokens;
    std::string digits;
    std::string words;
    auto flush_words = [&]() {
        if (!words.empty()) {
            tokens.push_back({false, ascii_lower(words), ""});
            words.clear();
        }
    };
    auto flush_digits = [&]() {
        if (!digits.empty()) {
            std::string trimmed = digits;
            std::size_t first = trimmed.find_first_not_of('0');
            trimmed = first == std::string::npos ? std::string("0") : trimmed.substr(first);
            tokens.push_back({true, digits, trimmed});
            digits.clear();
        }
    };
    for (char ch : text) {
        if (ch >= '0' && ch <= '9') {
            flush_words();
            digits.push_back(ch);
        } else {
            flush_digits();
            words.push_back(ch);
        }
    }
    flush_digits();
    flush_words();
    return tokens;
}

inline int compare_natural(const std::string& left, const std::string& right) {
    std::vector<SortToken> a = tokenize(left);
    std::vector<SortToken> b = tokenize(right);
    std::size_t limit = std::min(a.size(), b.size());
    for (std::size_t i = 0; i < limit; ++i) {
        const SortToken& lt = a[i];
        const SortToken& rt = b[i];
        if (lt.is_number && !rt.is_number) return -1;
        if (!lt.is_number && rt.is_number) return 1;
        if (lt.is_number) {
            if (lt.trimmed.size() != rt.trimmed.size()) {
                return lt.trimmed.size() < rt.trimmed.size() ? -1 : 1;
            }
            if (lt.trimmed != rt.trimmed) return lt.trimmed < rt.trimmed ? -1 : 1;
            if (lt.raw != rt.raw) return lt.raw < rt.raw ? -1 : 1;
        } else if (lt.raw != rt.raw) {
            return lt.raw < rt.raw ? -1 : 1;
        }
    }
    if (a.size() == b.size()) return 0;
    return a.size() < b.size() ? -1 : 1;
}

inline std::vector<CatalogProvider> sort_providers(
    const std::vector<CatalogProvider>& providers,
    const std::vector<std::string>& featured) {
    std::vector<std::pair<std::string, int>> ranks;
    for (std::size_t i = 0; i < featured.size(); ++i) {
        std::string key = ascii_lower(featured[i]);
        ranks.emplace_back(key, static_cast<int>(i));
    }
    auto rank_of = [&](const std::string& id) {
        std::string key = ascii_lower(id);
        for (const auto& entry : ranks) {
            if (entry.first == key) return entry.second;
        }
        return 2147483647;
    };
    std::vector<CatalogProvider> out = providers;
    std::stable_sort(out.begin(), out.end(), [&](const CatalogProvider& a, const CatalogProvider& b) {
        int ra = rank_of(a.id);
        int rb = rank_of(b.id);
        if (ra != rb) return ra < rb;
        int by_name = compare_natural(a.name, b.name);
        if (by_name != 0) return by_name < 0;
        return a.id < b.id;
    });
    return out;
}

inline std::vector<CatalogModel> models_in_stable_order(const std::vector<CatalogModel>& models) {
    std::vector<CatalogModel> out = models;
    std::stable_sort(out.begin(), out.end(), [](const CatalogModel& a, const CatalogModel& b) {
        std::string ra = a.release_date.value_or("");
        std::string rb = b.release_date.value_or("");
        if (ra != rb) {
            if (ra.empty()) return false;
            if (rb.empty()) return true;
            return rb < ra;
        }
        int by_family = compare_natural(a.family.value_or(""), b.family.value_or(""));
        if (by_family != 0) return by_family < 0;
        int by_name = compare_natural(a.name, b.name);
        if (by_name != 0) return by_name < 0;
        return a.id < b.id;
    });
    return out;
}

namespace detail {

inline std::vector<std::string> string_list(const Value* value) {
    std::vector<std::string> out;
    if (!value || !value->is_array()) return out;
    for (const auto& item : value->array()) {
        if (item.is_string()) out.push_back(item.string());
    }
    return out;
}

inline bool has_prefix(const std::vector<std::string>& values, const std::string& prefix) {
    for (const auto& value : values) {
        if (ascii_lower(value).rfind(prefix, 0) == 0) return true;
    }
    return false;
}

inline std::optional<std::string> non_empty_text(const std::string& text) {
    std::size_t first = text.find_first_not_of(" \t\n\r");
    if (first == std::string::npos) return std::nullopt;
    return text;
}

inline std::optional<std::string> non_empty(const Value* value) {
    if (!value || !value->is_string()) return std::nullopt;
    return non_empty_text(value->string());
}

inline std::optional<long long> number(const Value* value) {
    if (!value) return std::nullopt;
    return value->optional_integer();
}

inline std::optional<double> real(const Value* value) {
    if (!value) return std::nullopt;
    return value->optional_number();
}

inline std::optional<std::string> text(const Value* value) {
    if (!value || !value->is_string()) return std::nullopt;
    return value->string();
}

inline std::optional<CatalogModel> decode_model(
    const std::string& provider_id,
    const std::string& key,
    const Value& fields) {
    std::optional<std::string> identity = non_empty_text(key);
    if (!identity) identity = non_empty(fields.find("id"));
    if (!identity) return std::nullopt;

    CatalogModel model;
    model.id = *identity;
    model.provider_id = provider_id;

    const Value* limit = fields.find("limit");
    const Value* cost = fields.find("cost");
    const Value* modalities = fields.find("modalities");
    const Value* input_modalities = modalities ? modalities->find("input") : nullptr;
    const Value* output_modalities = modalities ? modalities->find("output") : nullptr;

    model.capabilities.input_modalities = string_list(input_modalities);
    model.capabilities.output_modalities = string_list(output_modalities);
    const Value* attachment = fields.find("attachment");
    model.capabilities.attachments = attachment && attachment->is_bool() && attachment->boolean();
    model.capabilities.vision =
        model.capabilities.attachments || has_prefix(model.capabilities.input_modalities, "image");

    const Value* options = fields.find("reasoning_options");
    if (options && options->is_array()) {
        for (const auto& option : options->array()) {
            std::vector<std::string> values = string_list(option.find("values"));
            model.capabilities.reasoning_efforts.insert(
                model.capabilities.reasoning_efforts.end(), values.begin(), values.end());
        }
    }

    model.name = text(fields.find("name")).value_or(model.id);
    model.family = non_empty(fields.find("family"));
    model.overview = non_empty(fields.find("description"));
    model.release_date = non_empty(fields.find("release_date"));
    model.last_updated = non_empty(fields.find("last_updated"));
    model.knowledge_cutoff = non_empty(fields.find("knowledge"));
    model.canonical_model_id = non_empty(fields.find("canonical_model_id"));
    model.status = model_status_from_raw(text(fields.find("status")));

    model.limits = ModelLimits{
        number(limit ? limit->find("context") : nullptr),
        number(limit ? limit->find("input") : nullptr),
        number(limit ? limit->find("output") : nullptr),
    };

    model.capabilities.reasoning =
        fields.find("reasoning") && fields.find("reasoning")->is_bool() && fields.find("reasoning")->boolean();
    model.capabilities.tool_calling =
        fields.find("tool_call") && fields.find("tool_call")->is_bool() && fields.find("tool_call")->boolean();
    model.capabilities.structured_output = fields.find("structured_output") &&
        fields.find("structured_output")->is_bool() && fields.find("structured_output")->boolean();
    model.capabilities.audio_input = has_prefix(model.capabilities.input_modalities, "audio");
    model.capabilities.audio_output = has_prefix(model.capabilities.output_modalities, "audio");
    model.capabilities.open_weights =
        fields.find("open_weights") && fields.find("open_weights")->is_bool() && fields.find("open_weights")->boolean();
    if (const Value* temperature = fields.find("temperature"); temperature && temperature->is_bool()) {
        model.capabilities.supports_temperature = temperature->boolean();
    }

    model.pricing = ModelPricing{
        real(cost ? cost->find("input") : nullptr),
        real(cost ? cost->find("output") : nullptr),
        real(cost ? cost->find("cache_read") : nullptr),
        real(cost ? cost->find("cache_write") : nullptr),
        real(cost ? cost->find("reasoning") : nullptr),
        real(cost ? cost->find("input_audio") : nullptr),
        real(cost ? cost->find("output_audio") : nullptr),
    };
    model.fields = fields;
    return model;
}

} // namespace detail

// Standalone, dependency-free catalog: no inference runtime, no API keys.
class ModelCatalog {
public:
    static ModelCatalog decode(const std::string& document) {
        Value root;
        try {
            root = Value::parse(document);
        } catch (const std::exception& error) {
            throw CatalogError(CatalogError::Kind::InvalidCatalog, std::string("Invalid JSON: ") + error.what());
        }
        if (!root.is_object()) {
            throw CatalogError(CatalogError::Kind::InvalidCatalog, "Catalog root is not an object");
        }
        const Value* providers = root.find("providers");
        if (!providers || !providers->is_object() || providers->object().empty()) {
            throw CatalogError(CatalogError::Kind::InvalidCatalog, "providers missing or empty");
        }

        std::string schema_version =
            detail::text(root.find("schemaVersion")).value_or(detail::text(root.find("version")).value_or("1.0"));
        std::string major = schema_version.substr(0, schema_version.find('.'));
        if (major != "1" && major != "2") {
            throw CatalogError(
                CatalogError::Kind::UnsupportedSchemaVersion,
                "Unsupported schema version: " + schema_version,
                schema_version);
        }

        std::vector<CatalogProvider> decoded;
        for (const auto& entry : providers->object()) {
            const Value& fields = entry.second;
            if (!fields.is_object()) continue;
            const Value* raw_models = fields.find("models");
            std::vector<CatalogModel> models;
            if (raw_models && raw_models->is_object()) {
                for (const auto& model_entry : raw_models->object()) {
                    if (!model_entry.second.is_object()) continue;
                    std::optional<CatalogModel> decoded_model =
                        detail::decode_model(entry.first, model_entry.first, model_entry.second);
                    if (decoded_model) models.push_back(std::move(*decoded_model));
                }
            }
            if (models.empty() && !fields.has("id")) continue;

            CatalogProvider provider;
            provider.id = detail::text(fields.find("id")).value_or(entry.first);
            provider.name = detail::text(fields.find("name")).value_or(provider.id);
            provider.base_url = detail::non_empty(fields.find("baseURL"));
            provider.documentation_url = detail::non_empty(fields.find("doc"));
            provider.environment_variable_names = detail::string_list(fields.find("env"));
            provider.models = models_in_stable_order(models);
            provider.fields = fields;
            decoded.push_back(std::move(provider));
        }
        if (decoded.empty()) {
            throw CatalogError(CatalogError::Kind::InvalidCatalog, "No valid provider entries");
        }

        CatalogRevision revision;
        revision.schema_version = schema_version;
        revision.catalog_revision = detail::text(root.find("catalogRevision"))
                                        .value_or(detail::text(root.find("catalogHash"))
                                                      .value_or(detail::text(root.find("generatedAt"))
                                                                    .value_or(detail::text(root.find("updatedAt"))
                                                                                  .value_or(""))));
        revision.catalog_hash = detail::text(root.find("catalogHash"));
        revision.source = detail::text(root.find("source"));
        revision.source_url = detail::text(root.find("sourceURL"));
        revision.source_hash = detail::text(root.find("sourceHash"));
        revision.generated_at = detail::text(root.find("generatedAt")).value_or(detail::text(root.find("updatedAt")).value_or(""));
        revision.source_fetched_at = detail::text(root.find("sourceFetchedAt"));

        std::vector<CatalogProvider> sorted = sort_providers(decoded, {});
        revision.total_providers = static_cast<int>(
            detail::number(root.find("totalProviders")).value_or(static_cast<long long>(sorted.size())));
        int model_total = 0;
        for (const auto& provider : sorted) model_total += static_cast<int>(provider.model_count());
        revision.total_models = static_cast<int>(detail::number(root.find("totalModels")).value_or(model_total));

        return ModelCatalog(std::move(revision), std::move(sorted), document);
    }

    const CatalogRevision& revision() const { return revision_; }
    const std::vector<CatalogProvider>& providers() const { return providers_; }
    const std::string& raw_document() const { return raw_; }

    std::size_t total_models() const {
        std::size_t total = 0;
        for (const auto& provider : providers_) total += provider.model_count();
        return total;
    }

    std::optional<CatalogProvider> provider(const std::string& provider_id) const {
        std::string key = ascii_lower(provider_id);
        for (const auto& provider : providers_) {
            if (ascii_lower(provider.id) == key) return provider;
        }
        return std::nullopt;
    }

    std::optional<CatalogModel> model(const std::string& provider_id, const std::string& model_id) const {
        std::optional<CatalogProvider> found = provider(provider_id);
        if (!found) return std::nullopt;
        std::string key = ascii_lower(model_id);
        for (const auto& model : found->models) {
            if (ascii_lower(model.id) == key) return model;
        }
        return std::nullopt;
    }

    std::optional<CatalogModel> model(const std::string& qualified_id) const {
        std::size_t slash = qualified_id.find('/');
        if (slash != std::string::npos) {
            return model(qualified_id.substr(0, slash), qualified_id.substr(slash + 1));
        }
        std::string key = ascii_lower(qualified_id);
        for (const auto& provider : providers_) {
            for (const auto& model : provider.models) {
                if (ascii_lower(model.id) == key) return model;
            }
        }
        return std::nullopt;
    }

    std::vector<CatalogProvider> sorted_providers(const std::vector<std::string>& featured = {}) const {
        return sort_providers(providers_, featured);
    }

    std::vector<CatalogModel> models_for_provider(const std::string& provider_id) const {
        std::optional<CatalogProvider> found = provider(provider_id);
        if (!found) return {};
        return models_in_stable_order(found->models);
    }

    std::vector<CatalogModel> all_models() const {
        std::vector<CatalogModel> out;
        for (const auto& provider : sorted_providers()) {
            std::vector<CatalogModel> ordered = models_in_stable_order(provider.models);
            out.insert(out.end(), ordered.begin(), ordered.end());
        }
        return out;
    }

    std::vector<CatalogModel> search(const std::string& query, std::optional<std::size_t> limit = std::nullopt) const {
        std::string needle = ascii_lower(query);
        needle.erase(0, needle.find_first_not_of(" \t\n\r"));
        needle.erase(std::min(needle.find_last_not_of(" \t\n\r") + 1, needle.size()));
        if (needle.empty()) {
            std::vector<CatalogModel> all = all_models();
            if (limit && all.size() > *limit) all.resize(*limit);
            return all;
        }
        std::vector<CatalogModel> matches;
        for (const auto& provider : sorted_providers()) {
            std::vector<CatalogModel> ordered = models_in_stable_order(provider.models);
            bool provider_matches =
                ascii_lower(provider.id).find(needle) != std::string::npos || ascii_lower(provider.name).find(needle) != std::string::npos;
            if (provider_matches) {
                matches.insert(matches.end(), ordered.begin(), ordered.end());
            } else {
                for (const auto& model : ordered) {
                    if (contains_any(needle, {model.id, model.name, model.family.value_or(""), model.overview.value_or("")})) {
                        matches.push_back(model);
                    }
                }
            }
            if (limit && matches.size() >= *limit) {
                matches.resize(*limit);
                return matches;
            }
        }
        return matches;
    }

    std::vector<CatalogModel> models_matching(const ModelFilter& filter) const {
        std::vector<CatalogModel> matches;
        for (const auto& provider : sorted_providers(filter.featured_provider_ids)) {
            if (filter.provider_id && ascii_lower(provider.id) != ascii_lower(*filter.provider_id)) continue;
            for (const auto& model : models_in_stable_order(provider.models)) {
                if (matches_filter(model, provider, filter)) matches.push_back(model);
            }
        }
        return matches;
    }

private:
    ModelCatalog(CatalogRevision revision, std::vector<CatalogProvider> providers, std::string raw)
        : revision_(std::move(revision)), providers_(std::move(providers)), raw_(std::move(raw)) {}

    static bool contains_any(const std::string& needle, const std::vector<std::string>& fields) {
        for (const auto& field : fields) {
            if (ascii_lower(field).find(needle) != std::string::npos) return true;
        }
        return false;
    }

    static bool matches_filter(const CatalogModel& model, const CatalogProvider& provider, const ModelFilter& filter) {
        if (!filter.include_deprecated && !is_selectable(model.status)) return false;
        if (filter.reasoning && model.capabilities.reasoning != *filter.reasoning) return false;
        if (filter.tool_calling && model.capabilities.tool_calling != *filter.tool_calling) return false;
        if (filter.structured_output && model.capabilities.structured_output != *filter.structured_output) return false;
        if (filter.vision && model.capabilities.vision != *filter.vision) return false;
        if (filter.attachments && model.capabilities.attachments != *filter.attachments) return false;
        if (filter.open_weights && model.capabilities.open_weights != *filter.open_weights) return false;
        if (filter.minimum_context_window) {
            long long context = model.limits.context.value_or(0);
            if (context < *filter.minimum_context_window) return false;
        }
        if (filter.minimum_max_output_tokens) {
            long long output = model.limits.output.value_or(0);
            if (output < *filter.minimum_max_output_tokens) return false;
        }
        if (filter.query) {
            std::string query = ascii_lower(*filter.query);
            if (!query.empty() &&
                !contains_any(query, {model.id, model.name, model.family.value_or(""), model.overview.value_or(""), provider.id, provider.name})) {
                return false;
            }
        }
        return true;
    }

    CatalogRevision revision_;
    std::vector<CatalogProvider> providers_;
    std::string raw_;
};

} // namespace lingxi::catalog
