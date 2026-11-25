package cn.lingxifox.modelsdk

import cn.lingxifox.modelsdk.CatalogTypes.CatalogModel
import cn.lingxifox.modelsdk.CatalogTypes.CatalogProvider
import java.util.Locale

/** Deterministic catalog ordering shared by every SDK (natural sort + featured-first). */
object CatalogOrdering {
    private data class Token(val isNumber: Boolean, val raw: String, val trimmed: String)

    private fun tokenize(text: String?): List<Token> {
        if (text.isNullOrEmpty()) return emptyList()
        val tokens = ArrayList<Token>()
        val digits = StringBuilder()
        val words = StringBuilder()
        for (ch in text) {
            if (ch in '0'..'9') {
                if (words.isNotEmpty()) {
                    tokens.add(Token(false, words.toString().lowercase(Locale.ROOT), ""))
                    words.setLength(0)
                }
                digits.append(ch)
            } else {
                if (digits.isNotEmpty()) {
                    val raw = digits.toString()
                    var trimmed = raw.trimStart('0')
                    if (trimmed.isEmpty()) trimmed = "0"
                    tokens.add(Token(true, raw, trimmed))
                    digits.setLength(0)
                }
                words.append(ch)
            }
        }
        if (digits.isNotEmpty()) {
            val raw = digits.toString()
            var trimmed = raw.trimStart('0')
            if (trimmed.isEmpty()) trimmed = "0"
            tokens.add(Token(true, raw, trimmed))
        }
        if (words.isNotEmpty()) {
            tokens.add(Token(false, words.toString().lowercase(Locale.ROOT), ""))
        }
        return tokens
    }

    fun compareNatural(a: String?, b: String?): Int {
        val left = tokenize(a ?: "")
        val right = tokenize(b ?: "")
        val limit = minOf(left.size, right.size)
        for (i in 0 until limit) {
            val lt = left[i]
            val rt = right[i]
            if (lt.isNumber && !rt.isNumber) return -1
            if (!lt.isNumber && rt.isNumber) return 1
            if (lt.isNumber) {
                val lenCmp = lt.trimmed.length.compareTo(rt.trimmed.length)
                if (lenCmp != 0) return lenCmp
                val valCmp = lt.trimmed.compareTo(rt.trimmed)
                if (valCmp != 0) return valCmp
                val rawCmp = lt.raw.compareTo(rt.raw)
                if (rawCmp != 0) return rawCmp
            } else {
                val txtCmp = lt.raw.compareTo(rt.raw)
                if (txtCmp != 0) return txtCmp
            }
        }
        return left.size.compareTo(right.size)
    }

    fun sortProviders(providers: List<CatalogProvider>, featured: List<String>?): List<CatalogProvider> {
        val ranks = HashMap<String, Int>()
        featured?.forEachIndexed { index, id -> ranks[id.trim().lowercase(Locale.ROOT)] = index }
        return providers.sortedWith { a, b ->
            val ra = ranks[a.id.trim().lowercase(Locale.ROOT)] ?: Int.MAX_VALUE
            val rb = ranks[b.id.trim().lowercase(Locale.ROOT)] ?: Int.MAX_VALUE
            if (ra != rb) return@sortedWith ra.compareTo(rb)
            val byName = compareNatural(a.name, b.name)
            if (byName != 0) return@sortedWith byName
            a.id.compareTo(b.id)
        }
    }

    fun modelsInStableOrder(models: List<CatalogModel>): List<CatalogModel> = models.sortedWith { a, b ->
        val ra = a.releaseDate ?: ""
        val rb = b.releaseDate ?: ""
        if (ra != rb) {
            if (ra.isEmpty()) return@sortedWith 1
            if (rb.isEmpty()) return@sortedWith -1
            return@sortedWith rb.compareTo(ra)
        }
        val byFamily = compareNatural(a.family, b.family)
        if (byFamily != 0) return@sortedWith byFamily
        val byName = compareNatural(a.name, b.name)
        if (byName != 0) return@sortedWith byName
        a.id.compareTo(b.id)
    }
}
