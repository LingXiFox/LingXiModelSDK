#pragma once

#include <cstdint>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace lingxi::json {

class Value;
using Array = std::vector<Value>;
using Object = std::vector<std::pair<std::string, Value>>;

// Minimal RFC 8259 value used by the standalone Catalog module and by the
// request/response plumbing that talks to the Rust core. Object members keep
// document order and unknown fields are preserved, which is what the catalog
// contract requires.
class Value {
public:
    enum class Kind { Null, Bool, Number, String, Array, Object };

    Value() = default;
    Value(std::nullptr_t) {}
    Value(bool value) : kind_(Kind::Bool), bool_(value) {}
    Value(int value) : kind_(Kind::Number), number_(value), integral_(true), integer_(value) {}
    Value(long value) : kind_(Kind::Number), number_(double(value)), integral_(true), integer_(value) {}
    Value(long long value) : kind_(Kind::Number), number_(double(value)), integral_(true), integer_(value) {}
    Value(double value) : kind_(Kind::Number), number_(value) {}
    Value(const char* value) : kind_(Kind::String), string_(value) {}
    Value(std::string value) : kind_(Kind::String), string_(std::move(value)) {}
    Value(Array value) : kind_(Kind::Array), array_(std::move(value)) {}
    Value(Object value) : kind_(Kind::Object), object_(std::move(value)) {}

    Kind kind() const { return kind_; }
    bool is_null() const { return kind_ == Kind::Null; }
    bool is_bool() const { return kind_ == Kind::Bool; }
    bool is_number() const { return kind_ == Kind::Number; }
    bool is_string() const { return kind_ == Kind::String; }
    bool is_array() const { return kind_ == Kind::Array; }
    bool is_object() const { return kind_ == Kind::Object; }

    const Array& array() const {
        require(Kind::Array, "array");
        return array_;
    }
    Array& array() {
        require(Kind::Array, "array");
        return array_;
    }
    const Object& object() const {
        require(Kind::Object, "object");
        return object_;
    }
    Object& object() {
        require(Kind::Object, "object");
        return object_;
    }
    const std::string& string() const {
        require(Kind::String, "string");
        return string_;
    }
    bool boolean() const {
        require(Kind::Bool, "bool");
        return bool_;
    }
    double number() const {
        require(Kind::Number, "number");
        return number_;
    }

    std::optional<std::string> optional_string() const {
        return kind_ == Kind::String ? std::optional<std::string>(string_) : std::nullopt;
    }
    std::optional<bool> optional_bool() const {
        return kind_ == Kind::Bool ? std::optional<bool>(bool_) : std::nullopt;
    }
    std::optional<double> optional_number() const {
        return kind_ == Kind::Number ? std::optional<double>(number_) : std::nullopt;
    }
    std::optional<long long> optional_integer() const {
        if (kind_ != Kind::Number) return std::nullopt;
        if (integral_) return integer_;
        double rounded = number_;
        long long as_integer = static_cast<long long>(rounded);
        if (double(as_integer) == rounded) return as_integer;
        return std::nullopt;
    }

    bool has(std::string_view key) const { return find(key) != nullptr; }

    const Value* find(std::string_view key) const {
        if (kind_ != Kind::Object) return nullptr;
        for (const auto& entry : object_) {
            if (entry.first == key) return &entry.second;
        }
        return nullptr;
    }

    const Value& at(std::string_view key) const {
        const Value* found = find(key);
        if (!found) throw std::runtime_error("missing JSON key: " + std::string(key));
        return *found;
    }

    const Value& at(std::string_view key, const Value& fallback) const {
        const Value* found = find(key);
        return found ? *found : fallback;
    }

    void set(std::string key, Value value) {
        if (kind_ != Kind::Object) {
            kind_ = Kind::Object;
            object_.clear();
        }
        for (auto& entry : object_) {
            if (entry.first == key) {
                entry.second = std::move(value);
                return;
            }
        }
        object_.emplace_back(std::move(key), std::move(value));
    }

    void push(Value value) {
        if (kind_ != Kind::Array) {
            kind_ = Kind::Array;
            array_.clear();
        }
        array_.push_back(std::move(value));
    }

    std::string dump() const;

    static Value parse(std::string_view text);

private:
    void require(Kind expected, const char* label) const {
        if (kind_ != expected) throw std::runtime_error(std::string("JSON value is not ") + label);
    }

    Kind kind_ = Kind::Null;
    bool bool_ = false;
    double number_ = 0.0;
    bool integral_ = false;
    long long integer_ = 0;
    std::string string_;
    Array array_;
    Object object_;
};

namespace detail {

class Parser {
public:
    explicit Parser(std::string_view text) : text_(text) {}

    Value parse_document() {
        skip_space();
        Value value = parse_value();
        skip_space();
        if (pos_ != text_.size()) fail("trailing content");
        return value;
    }

private:
    [[noreturn]] void fail(const char* what) const {
        throw std::runtime_error(std::string("invalid JSON at offset ") + std::to_string(pos_) + ": " + what);
    }

    void skip_space() {
        while (pos_ < text_.size()) {
            char ch = text_[pos_];
            if (ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r') {
                ++pos_;
            } else {
                break;
            }
        }
    }

    bool consume(char expected) {
        if (pos_ < text_.size() && text_[pos_] == expected) {
            ++pos_;
            return true;
        }
        return false;
    }

    void expect(char expected) {
        if (!consume(expected)) fail("unexpected character");
    }

    Value parse_value() {
        if (pos_ >= text_.size()) fail("unexpected end of input");
        char ch = text_[pos_];
        switch (ch) {
            case '{': return parse_object();
            case '[': return parse_array();
            case '"': return Value(parse_string());
            case 't': consume_literal("true"); return Value(true);
            case 'f': consume_literal("false"); return Value(false);
            case 'n': consume_literal("null"); return Value();
            default: return parse_number();
        }
    }

    void consume_literal(std::string_view literal) {
        if (text_.substr(pos_, literal.size()) != literal) fail("bad literal");
        pos_ += literal.size();
    }

    Value parse_object() {
        expect('{');
        Object members;
        skip_space();
        if (consume('}')) return Value(std::move(members));
        while (true) {
            skip_space();
            if (pos_ >= text_.size() || text_[pos_] != '"') fail("object key must be a string");
            std::string key = parse_string();
            skip_space();
            expect(':');
            skip_space();
            members.emplace_back(std::move(key), parse_value());
            skip_space();
            if (consume(',')) continue;
            expect('}');
            break;
        }
        return Value(std::move(members));
    }

    Value parse_array() {
        expect('[');
        Array items;
        skip_space();
        if (consume(']')) return Value(std::move(items));
        while (true) {
            skip_space();
            items.push_back(parse_value());
            skip_space();
            if (consume(',')) continue;
            expect(']');
            break;
        }
        return Value(std::move(items));
    }

    static void append_utf8(std::string& out, unsigned int code) {
        if (code <= 0x7F) {
            out.push_back(char(code));
        } else if (code <= 0x7FF) {
            out.push_back(char(0xC0 | (code >> 6)));
            out.push_back(char(0x80 | (code & 0x3F)));
        } else if (code <= 0xFFFF) {
            out.push_back(char(0xE0 | (code >> 12)));
            out.push_back(char(0x80 | ((code >> 6) & 0x3F)));
            out.push_back(char(0x80 | (code & 0x3F)));
        } else {
            out.push_back(char(0xF0 | (code >> 18)));
            out.push_back(char(0x80 | ((code >> 12) & 0x3F)));
            out.push_back(char(0x80 | ((code >> 6) & 0x3F)));
            out.push_back(char(0x80 | (code & 0x3F)));
        }
    }

    unsigned int parse_hex4() {
        if (pos_ + 4 > text_.size()) fail("truncated \\u escape");
        unsigned int value = 0;
        for (int i = 0; i < 4; ++i) {
            char ch = text_[pos_++];
            value <<= 4;
            if (ch >= '0' && ch <= '9') value |= unsigned(ch - '0');
            else if (ch >= 'a' && ch <= 'f') value |= unsigned(ch - 'a' + 10);
            else if (ch >= 'A' && ch <= 'F') value |= unsigned(ch - 'A' + 10);
            else fail("bad \\u escape");
        }
        return value;
    }

    std::string parse_string() {
        expect('"');
        std::string out;
        while (true) {
            if (pos_ >= text_.size()) fail("unterminated string");
            char ch = text_[pos_++];
            if (ch == '"') break;
            if (ch != '\\') {
                out.push_back(ch);
                continue;
            }
            if (pos_ >= text_.size()) fail("unterminated escape");
            char esc = text_[pos_++];
            switch (esc) {
                case '"': out.push_back('"'); break;
                case '\\': out.push_back('\\'); break;
                case '/': out.push_back('/'); break;
                case 'b': out.push_back('\b'); break;
                case 'f': out.push_back('\f'); break;
                case 'n': out.push_back('\n'); break;
                case 'r': out.push_back('\r'); break;
                case 't': out.push_back('\t'); break;
                case 'u': {
                    unsigned int code = parse_hex4();
                    if (code >= 0xD800 && code <= 0xDBFF && pos_ + 6 <= text_.size() &&
                        text_[pos_] == '\\' && text_[pos_ + 1] == 'u') {
                        pos_ += 2;
                        unsigned int low = parse_hex4();
                        if (low >= 0xDC00 && low <= 0xDFFF) {
                            code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                        }
                    }
                    append_utf8(out, code);
                    break;
                }
                default: fail("bad escape");
            }
        }
        return out;
    }

    Value parse_number() {
        std::size_t start = pos_;
        if (pos_ < text_.size() && (text_[pos_] == '-')) ++pos_;
        bool saw_digit = false;
        while (pos_ < text_.size() && text_[pos_] >= '0' && text_[pos_] <= '9') {
            ++pos_;
            saw_digit = true;
        }
        bool is_integral = true;
        if (pos_ < text_.size() && text_[pos_] == '.') {
            is_integral = false;
            ++pos_;
            while (pos_ < text_.size() && text_[pos_] >= '0' && text_[pos_] <= '9') ++pos_;
        }
        if (pos_ < text_.size() && (text_[pos_] == 'e' || text_[pos_] == 'E')) {
            is_integral = false;
            ++pos_;
            if (pos_ < text_.size() && (text_[pos_] == '+' || text_[pos_] == '-')) ++pos_;
            while (pos_ < text_.size() && text_[pos_] >= '0' && text_[pos_] <= '9') ++pos_;
        }
        if (!saw_digit) fail("bad number");
        std::string token(text_.substr(start, pos_ - start));
        if (is_integral) {
            try {
                long long integer = std::stoll(token);
                return Value(integer);
            } catch (const std::exception&) {
                // Fall through to the floating point representation.
            }
        }
        try {
            return Value(std::stod(token));
        } catch (const std::exception&) {
            fail("bad number");
        }
    }

    std::string_view text_;
    std::size_t pos_ = 0;
};

inline void escape_string(std::string& out, const std::string& value) {
    out.push_back('"');
    for (char ch : value) {
        unsigned char byte = static_cast<unsigned char>(ch);
        switch (ch) {
            case '"': out += "\\\""; break;
            case '\\': out += "\\\\"; break;
            case '\b': out += "\\b"; break;
            case '\f': out += "\\f"; break;
            case '\n': out += "\\n"; break;
            case '\r': out += "\\r"; break;
            case '\t': out += "\\t"; break;
            default:
                if (byte < 0x20) {
                    static const char* hex = "0123456789abcdef";
                    out += "\\u00";
                    out.push_back(hex[(byte >> 4) & 0xF]);
                    out.push_back(hex[byte & 0xF]);
                } else {
                    out.push_back(ch);
                }
        }
    }
    out.push_back('"');
}

inline void dump_value(std::string& out, const Value& value) {
    switch (value.kind()) {
        case Value::Kind::Null: out += "null"; break;
        case Value::Kind::Bool: out += value.boolean() ? "true" : "false"; break;
        case Value::Kind::Number: {
            std::optional<long long> integer = value.optional_integer();
            if (integer) {
                out += std::to_string(*integer);
            } else {
                out += std::to_string(value.number());
            }
            break;
        }
        case Value::Kind::String: escape_string(out, value.string()); break;
        case Value::Kind::Array: {
            out.push_back('[');
            bool first = true;
            for (const auto& item : value.array()) {
                if (!first) out.push_back(',');
                first = false;
                dump_value(out, item);
            }
            out.push_back(']');
            break;
        }
        case Value::Kind::Object: {
            out.push_back('{');
            bool first = true;
            for (const auto& entry : value.object()) {
                if (!first) out.push_back(',');
                first = false;
                escape_string(out, entry.first);
                out.push_back(':');
                dump_value(out, entry.second);
            }
            out.push_back('}');
            break;
        }
    }
}

} // namespace detail

inline std::string Value::dump() const {
    std::string out;
    detail::dump_value(out, *this);
    return out;
}

inline Value Value::parse(std::string_view text) {
    detail::Parser parser(text);
    return parser.parse_document();
}

} // namespace lingxi::json
