package cn.lingxifox.modelsdk

/**
 * Minimal RFC 8259 JSON codec.
 *
 * Values are [Map] (insertion-ordered), [List], [String], [Boolean], [Long],
 * [Double] and null - exactly the shapes exchanged with the Rust core.
 */
object Json {
    fun write(value: Any?): String {
        val out = StringBuilder()
        writeValue(out, value)
        return out.toString()
    }

    private fun writeValue(out: StringBuilder, value: Any?) {
        when (value) {
            null -> out.append("null")
            is String -> writeString(out, value)
            is Boolean -> out.append(if (value) "true" else "false")
            is Double -> {
                require(!value.isNaN() && !value.isInfinite()) { "JSON cannot represent $value" }
                out.append(value.toString())
            }
            is Float -> {
                require(!value.isNaN() && !value.isInfinite()) { "JSON cannot represent $value" }
                out.append(value.toString())
            }
            is Number -> out.append(value.toString())
            is Map<*, *> -> {
                out.append('{')
                var first = true
                for ((key, item) in value) {
                    if (!first) out.append(',')
                    first = false
                    writeString(out, key.toString())
                    out.append(':')
                    writeValue(out, item)
                }
                out.append('}')
            }
            is Iterable<*> -> {
                out.append('[')
                var first = true
                for (item in value) {
                    if (!first) out.append(',')
                    first = false
                    writeValue(out, item)
                }
                out.append(']')
            }
            is Array<*> -> {
                out.append('[')
                value.forEachIndexed { index, item ->
                    if (index > 0) out.append(',')
                    writeValue(out, item)
                }
                out.append(']')
            }
            else -> writeString(out, value.toString())
        }
    }

    private fun writeString(out: StringBuilder, value: String) {
        out.append('"')
        for (ch in value) {
            when (ch) {
                '"' -> out.append("\\\"")
                '\\' -> out.append("\\\\")
                '\n' -> out.append("\\n")
                '\r' -> out.append("\\r")
                '\t' -> out.append("\\t")
                '\b' -> out.append("\\b")
                '\u000c' -> out.append("\\f")
                else -> if (ch < ' ') out.append("\\u%04x".format(ch.code)) else out.append(ch)
            }
        }
        out.append('"')
    }

    fun parse(text: String): Any? {
        val parser = Parser(text)
        parser.skipWhitespace()
        val value = parser.readValue()
        parser.skipWhitespace()
        if (parser.pos != text.length) {
            throw IllegalArgumentException("Trailing JSON content at position ${parser.pos}")
        }
        return value
    }

    private class Parser(private val src: String) {
        var pos: Int = 0

        fun skipWhitespace() {
            while (pos < src.length && src[pos].let { it == ' ' || it == '\t' || it == '\r' || it == '\n' }) pos++
        }

        fun readValue(): Any? {
            skipWhitespace()
            if (pos >= src.length) throw IllegalArgumentException("Unexpected end of JSON")
            return when (val ch = src[pos]) {
                '{' -> readObject()
                '[' -> readArray()
                '"' -> readString()
                't', 'f' -> readBoolean()
                'n' -> readNull()
                else -> {
                    if (ch == '-' || ch in '0'..'9') readNumber()
                    else throw IllegalArgumentException("Unexpected char '$ch' at $pos")
                }
            }
        }

        private fun readObject(): Map<String, Any?> {
            pos++
            skipWhitespace()
            val map = LinkedHashMap<String, Any?>()
            if (pos < src.length && src[pos] == '}') {
                pos++
                return map
            }
            while (true) {
                skipWhitespace()
                if (pos >= src.length || src[pos] != '"') throw IllegalArgumentException("Expected string key at $pos")
                val key = readString()
                skipWhitespace()
                if (pos >= src.length || src[pos] != ':') throw IllegalArgumentException("Expected ':' at $pos")
                pos++
                map[key] = readValue()
                skipWhitespace()
                if (pos >= src.length) throw IllegalArgumentException("Unterminated object")
                val next = src[pos++]
                if (next == '}') return map
                if (next != ',') throw IllegalArgumentException("Expected ',' or '}' at ${pos - 1}")
            }
        }

        private fun readArray(): List<Any?> {
            pos++
            skipWhitespace()
            val list = ArrayList<Any?>()
            if (pos < src.length && src[pos] == ']') {
                pos++
                return list
            }
            while (true) {
                list.add(readValue())
                skipWhitespace()
                if (pos >= src.length) throw IllegalArgumentException("Unterminated array")
                val next = src[pos++]
                if (next == ']') return list
                if (next != ',') throw IllegalArgumentException("Expected ',' or ']' at ${pos - 1}")
            }
        }

        private fun readString(): String {
            pos++
            val sb = StringBuilder()
            while (pos < src.length) {
                val ch = src[pos++]
                if (ch == '"') return sb.toString()
                if (ch == '\\') {
                    if (pos >= src.length) throw IllegalArgumentException("Unterminated escape")
                    when (val esc = src[pos++]) {
                        '"', '\\', '/' -> sb.append(esc)
                        'b' -> sb.append('\b')
                        'f' -> sb.append('\u000c')
                        'n' -> sb.append('\n')
                        'r' -> sb.append('\r')
                        't' -> sb.append('\t')
                        'u' -> {
                            if (pos + 4 > src.length) throw IllegalArgumentException("Invalid unicode escape")
                            val code = src.substring(pos, pos + 4).toInt(16)
                            sb.append(code.toChar())
                            pos += 4
                        }
                        else -> throw IllegalArgumentException("Invalid escape \\$esc")
                    }
                } else {
                    sb.append(ch)
                }
            }
            throw IllegalArgumentException("Unterminated string")
        }

        private fun readBoolean(): Boolean {
            if (src.startsWith("true", pos)) {
                pos += 4
                return true
            }
            if (src.startsWith("false", pos)) {
                pos += 5
                return false
            }
            throw IllegalArgumentException("Invalid boolean at $pos")
        }

        private fun readNull(): Any? {
            if (src.startsWith("null", pos)) {
                pos += 4
                return null
            }
            throw IllegalArgumentException("Invalid null at $pos")
        }

        private fun readNumber(): Number {
            val start = pos
            if (src[pos] == '-') pos++
            while (pos < src.length && src[pos] in '0'..'9') pos++
            var isFloat = false
            if (pos < src.length && src[pos] == '.') {
                isFloat = true
                pos++
                while (pos < src.length && src[pos] in '0'..'9') pos++
            }
            if (pos < src.length && (src[pos] == 'e' || src[pos] == 'E')) {
                isFloat = true
                pos++
                if (pos < src.length && (src[pos] == '+' || src[pos] == '-')) pos++
                while (pos < src.length && src[pos] in '0'..'9') pos++
            }
            val raw = src.substring(start, pos)
            return if (isFloat) raw.toDouble() else raw.toLong()
        }
    }
}
