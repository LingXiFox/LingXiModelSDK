package cn.lingxifox.modelsdk;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Minimal RFC 8259 JSON parser preserving exact numbers, booleans, nulls, and object insertion order. */
public final class JsonParser {
    private final String src;
    private int pos;

    private JsonParser(String src) {
        this.src = src;
        this.pos = 0;
    }

    public static Object parse(String json) {
        if (json == null) {
            throw new IllegalArgumentException("null json");
        }
        JsonParser p = new JsonParser(json);
        p.skipWhitespace();
        Object value = p.readValue();
        p.skipWhitespace();
        if (p.pos != p.src.length()) {
            throw new IllegalArgumentException("Trailing JSON content at position " + p.pos);
        }
        return value;
    }

    private void skipWhitespace() {
        while (pos < src.length()) {
            char c = src.charAt(pos);
            if (c == ' ' || c == '\t' || c == '\r' || c == '\n') {
                pos++;
            } else {
                break;
            }
        }
    }

    private Object readValue() {
        skipWhitespace();
        if (pos >= src.length()) {
            throw new IllegalArgumentException("Unexpected end of JSON");
        }
        char c = src.charAt(pos);
        return switch (c) {
            case '{' -> readObject();
            case '[' -> readArray();
            case '"' -> readString();
            case 't', 'f' -> readBoolean();
            case 'n' -> readNull();
            default -> {
                if (c == '-' || (c >= '0' && c <= '9')) {
                    yield readNumber();
                }
                throw new IllegalArgumentException("Unexpected char '" + c + "' at " + pos);
            }
        };
    }

    private Map<String, Object> readObject() {
        pos++; // '{'
        skipWhitespace();
        Map<String, Object> map = new LinkedHashMap<>();
        if (pos < src.length() && src.charAt(pos) == '}') {
            pos++;
            return map;
        }
        while (true) {
            skipWhitespace();
            if (pos >= src.length() || src.charAt(pos) != '"') {
                throw new IllegalArgumentException("Expected string key at " + pos);
            }
            String key = readString();
            skipWhitespace();
            if (pos >= src.length() || src.charAt(pos) != ':') {
                throw new IllegalArgumentException("Expected ':' at " + pos);
            }
            pos++;
            Object val = readValue();
            map.put(key, val);
            skipWhitespace();
            if (pos >= src.length()) {
                throw new IllegalArgumentException("Unterminated object");
            }
            char next = src.charAt(pos++);
            if (next == '}') {
                return map;
            }
            if (next != ',') {
                throw new IllegalArgumentException("Expected ',' or '}' at " + (pos - 1));
            }
        }
    }

    private List<Object> readArray() {
        pos++; // '['
        skipWhitespace();
        List<Object> list = new ArrayList<>();
        if (pos < src.length() && src.charAt(pos) == ']') {
            pos++;
            return list;
        }
        while (true) {
            list.add(readValue());
            skipWhitespace();
            if (pos >= src.length()) {
                throw new IllegalArgumentException("Unterminated array");
            }
            char next = src.charAt(pos++);
            if (next == ']') {
                return list;
            }
            if (next != ',') {
                throw new IllegalArgumentException("Expected ',' or ']' at " + (pos - 1));
            }
        }
    }

    private String readString() {
        pos++; // '"'
        StringBuilder sb = new StringBuilder();
        while (pos < src.length()) {
            char c = src.charAt(pos++);
            if (c == '"') {
                return sb.toString();
            }
            if (c == '\\') {
                if (pos >= src.length()) {
                    throw new IllegalArgumentException("Unterminated escape");
                }
                char esc = src.charAt(pos++);
                switch (esc) {
                    case '"', '\\', '/' -> sb.append(esc);
                    case 'b' -> sb.append('\b');
                    case 'f' -> sb.append('\f');
                    case 'n' -> sb.append('\n');
                    case 'r' -> sb.append('\r');
                    case 't' -> sb.append('\t');
                    case 'u' -> {
                        if (pos + 4 > src.length()) {
                            throw new IllegalArgumentException("Invalid unicode escape");
                        }
                        int code = Integer.parseInt(src.substring(pos, pos + 4), 16);
                        sb.append((char) code);
                        pos += 4;
                    }
                    default -> throw new IllegalArgumentException("Invalid escape \\" + esc);
                }
            } else {
                sb.append(c);
            }
        }
        throw new IllegalArgumentException("Unterminated string");
    }

    private Boolean readBoolean() {
        if (src.startsWith("true", pos)) {
            pos += 4;
            return Boolean.TRUE;
        }
        if (src.startsWith("false", pos)) {
            pos += 5;
            return Boolean.FALSE;
        }
        throw new IllegalArgumentException("Invalid boolean at " + pos);
    }

    private Object readNull() {
        if (src.startsWith("null", pos)) {
            pos += 4;
            return null;
        }
        throw new IllegalArgumentException("Invalid null at " + pos);
    }

    private Number readNumber() {
        int start = pos;
        if (src.charAt(pos) == '-') {
            pos++;
        }
        while (pos < src.length() && Character.isDigit(src.charAt(pos))) {
            pos++;
        }
        boolean isFloat = false;
        if (pos < src.length() && src.charAt(pos) == '.') {
            isFloat = true;
            pos++;
            while (pos < src.length() && Character.isDigit(src.charAt(pos))) {
                pos++;
            }
        }
        if (pos < src.length() && (src.charAt(pos) == 'e' || src.charAt(pos) == 'E')) {
            isFloat = true;
            pos++;
            if (pos < src.length() && (src.charAt(pos) == '+' || src.charAt(pos) == '-')) {
                pos++;
            }
            while (pos < src.length() && Character.isDigit(src.charAt(pos))) {
                pos++;
            }
        }
        String raw = src.substring(start, pos);
        if (isFloat) {
            return Double.parseDouble(raw);
        }
        long val = Long.parseLong(raw);
        return val;
    }
}
