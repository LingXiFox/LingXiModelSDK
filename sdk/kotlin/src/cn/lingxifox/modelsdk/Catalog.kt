package cn.lingxifox.modelsdk

import cn.lingxifox.modelsdk.CatalogTypes.CatalogModel
import cn.lingxifox.modelsdk.CatalogTypes.CatalogProvider
import cn.lingxifox.modelsdk.CatalogTypes.CatalogRevision
import cn.lingxifox.modelsdk.CatalogTypes.ModelFilter
import java.util.Locale

/**
 * Immutable model catalog with lookup, search and filtering.
 *
 * This entry point is independent of the provider clients: it needs no API key
 * and no Rust core.
 */
class LingXiModelCatalog(
    val revision: CatalogRevision,
    private val providerList: List<CatalogProvider>,
    private val rawDocument: ByteArray?,
) {
    val providers: List<CatalogProvider> get() = providerList

    val rawDocumentBytes: ByteArray? get() = rawDocument?.copyOf()

    val totalModels: Int get() = providerList.sumOf { it.modelCount }

    fun provider(providerID: String): CatalogProvider? {
        val key = norm(providerID)
        return providerList.firstOrNull { norm(it.id) == key }
    }

    fun model(providerID: String, modelID: String): CatalogModel? {
        val prov = provider(providerID) ?: return null
        val key = norm(modelID)
        return prov.models.firstOrNull { norm(it.id) == key }
    }

    /** Looks a model up by id, accepting a `provider/model` qualified id. */
    fun model(id: String): CatalogModel? {
        val slash = id.indexOf('/')
        if (slash >= 0) return model(id.substring(0, slash), id.substring(slash + 1))
        val key = norm(id)
        for (prov in providerList) {
            for (m in prov.models) {
                if (norm(m.id) == key) return m
            }
        }
        return null
    }

    fun sortedProviders(featured: List<String> = emptyList()): List<CatalogProvider> =
        CatalogOrdering.sortProviders(providerList, featured)

    fun modelsForProvider(providerID: String): List<CatalogModel> =
        provider(providerID)?.let { CatalogOrdering.modelsInStableOrder(it.models) } ?: emptyList()

    fun allModels(): List<CatalogModel> {
        val out = ArrayList<CatalogModel>()
        for (prov in sortedProviders()) {
            out.addAll(CatalogOrdering.modelsInStableOrder(prov.models))
        }
        return out
    }

    fun search(query: String, limit: Int? = null): List<CatalogModel> {
        val needle = norm(query)
        if (needle.isEmpty()) {
            val all = allModels()
            return if (limit != null && all.size > limit) all.subList(0, limit) else all
        }
        val matches = ArrayList<CatalogModel>()
        for (prov in sortedProviders()) {
            val ordered = CatalogOrdering.modelsInStableOrder(prov.models)
            if (norm(prov.id).contains(needle) || norm(prov.name).contains(needle)) {
                matches.addAll(ordered)
            } else {
                for (m in ordered) {
                    if (containsAny(needle, m.id, m.name, m.family, m.overview)) matches.add(m)
                }
            }
            if (limit != null && matches.size >= limit) return matches.subList(0, limit).toList()
        }
        return matches
    }

    fun modelsMatching(filter: ModelFilter): List<CatalogModel> {
        val matches = ArrayList<CatalogModel>()
        for (prov in sortedProviders(filter.featuredProviderIDs)) {
            if (filter.providerID != null && norm(prov.id) != norm(filter.providerID)) continue
            for (m in CatalogOrdering.modelsInStableOrder(prov.models)) {
                if (matchesFilter(m, prov, filter)) matches.add(m)
            }
        }
        return matches
    }

    private fun matchesFilter(m: CatalogModel, prov: CatalogProvider, f: ModelFilter): Boolean {
        if (!f.includeDeprecated && !m.status.isSelectable) return false
        val caps = m.capabilities
        if (f.reasoning != null && caps.reasoning != f.reasoning) return false
        if (f.toolCalling != null && caps.toolCalling != f.toolCalling) return false
        if (f.structuredOutput != null && caps.structuredOutput != f.structuredOutput) return false
        if (f.vision != null && caps.vision != f.vision) return false
        if (f.attachments != null && caps.attachments != f.attachments) return false
        if (f.openWeights != null && caps.openWeights != f.openWeights) return false
        if (f.minimumContextWindow != null) {
            val ctx = m.limits.context ?: 0L
            if (ctx < f.minimumContextWindow) return false
        }
        if (f.minimumMaxOutputTokens != null) {
            val out = m.limits.output ?: 0L
            if (out < f.minimumMaxOutputTokens) return false
        }
        if (f.query != null) {
            val q = norm(f.query)
            if (q.isNotEmpty() && !containsAny(q, m.id, m.name, m.family, m.overview, prov.id, prov.name)) {
                return false
            }
        }
        return true
    }

    companion object {
        fun decode(data: ByteArray): LingXiModelCatalog = CatalogDecoder.decode(data)

        /** Validates that [jsonText] is a decodable catalog document. */
        fun validateSchemaV2(jsonText: String) {
            decode(jsonText.toByteArray(Charsets.UTF_8))
        }

        private fun norm(text: String?): String = text?.trim()?.lowercase(Locale.ROOT) ?: ""

        private fun containsAny(needle: String, vararg fields: String?): Boolean =
            fields.any { it != null && norm(it).contains(needle) }
    }
}
