package cn.lingxifox.modelsdk

import java.time.Instant

/** Catalog value types, mirroring the published `models.json` schema. */
object CatalogTypes {
    enum class ModelStatus {
        ACTIVE,
        PREVIEW,
        DEPRECATED,
        RETIRED,
        UNKNOWN;

        val isSelectable: Boolean
            get() = this != DEPRECATED && this != RETIRED

        companion object {
            fun fromRaw(raw: String?): ModelStatus = when (raw) {
                "deprecated", "retired" -> DEPRECATED
                "beta", "preview" -> PREVIEW
                "active" -> ACTIVE
                else -> UNKNOWN
            }
        }
    }

    data class ModelLimits(val context: Long?, val input: Long?, val output: Long?)

    data class ModelCapabilities(
        val reasoning: Boolean,
        val reasoningEfforts: List<String>,
        val toolCalling: Boolean,
        val structuredOutput: Boolean,
        val attachments: Boolean,
        val vision: Boolean,
        val audioInput: Boolean,
        val audioOutput: Boolean,
        val inputModalities: List<String>,
        val outputModalities: List<String>,
        val openWeights: Boolean,
        val supportsTemperature: Boolean?,
    )

    data class ModelPricing(
        val input: Double?,
        val output: Double?,
        val cacheRead: Double?,
        val cacheWrite: Double?,
        val reasoning: Double?,
        val inputAudio: Double?,
        val outputAudio: Double?,
    ) {
        val isUnknown: Boolean
            get() = input == null && output == null && cacheRead == null && cacheWrite == null &&
                reasoning == null && inputAudio == null && outputAudio == null
    }

    data class CatalogModel(
        val id: String,
        val providerID: String,
        val name: String,
        val family: String?,
        val overview: String?,
        val releaseDate: String?,
        val lastUpdated: String?,
        val knowledgeCutoff: String?,
        val canonicalModelID: String?,
        val status: ModelStatus,
        val limits: ModelLimits,
        val capabilities: ModelCapabilities,
        val pricing: ModelPricing,
        val fields: Map<String, Any?>,
    ) {
        val qualifiedID: String get() = "$providerID/$id"

        val contextWindow: Long? get() = limits.context

        val maxOutputTokens: Long? get() = limits.output
    }

    data class CatalogProvider(
        val id: String,
        val name: String,
        val baseURL: String?,
        val documentationURL: String?,
        val environmentVariableNames: List<String>,
        val models: List<CatalogModel>,
        val fields: Map<String, Any?>,
    ) {
        val modelCount: Int get() = models.size
    }

    data class CatalogRevision(
        val schemaVersion: String,
        val catalogRevision: String,
        val catalogHash: String?,
        val source: String?,
        val sourceURL: String?,
        val sourceHash: String?,
        val generatedAt: Instant?,
        val sourceFetchedAt: Instant?,
        val totalProviders: Int,
        val totalModels: Int,
    )

    data class ModelFilter(
        val providerID: String? = null,
        val query: String? = null,
        val reasoning: Boolean? = null,
        val toolCalling: Boolean? = null,
        val structuredOutput: Boolean? = null,
        val vision: Boolean? = null,
        val attachments: Boolean? = null,
        val openWeights: Boolean? = null,
        val minimumContextWindow: Long? = null,
        val minimumMaxOutputTokens: Long? = null,
        val includeDeprecated: Boolean = false,
        val featuredProviderIDs: List<String> = emptyList(),
    ) {
        class Builder {
            private var providerID: String? = null
            private var query: String? = null
            private var reasoning: Boolean? = null
            private var toolCalling: Boolean? = null
            private var structuredOutput: Boolean? = null
            private var vision: Boolean? = null
            private var attachments: Boolean? = null
            private var openWeights: Boolean? = null
            private var minimumContextWindow: Long? = null
            private var minimumMaxOutputTokens: Long? = null
            private var includeDeprecated: Boolean = false
            private var featuredProviderIDs: List<String> = emptyList()

            fun providerID(value: String?) = apply { providerID = value }
            fun query(value: String?) = apply { query = value }
            fun reasoning(value: Boolean?) = apply { reasoning = value }
            fun toolCalling(value: Boolean?) = apply { toolCalling = value }
            fun structuredOutput(value: Boolean?) = apply { structuredOutput = value }
            fun vision(value: Boolean?) = apply { vision = value }
            fun attachments(value: Boolean?) = apply { attachments = value }
            fun openWeights(value: Boolean?) = apply { openWeights = value }
            fun minimumContextWindow(value: Long?) = apply { minimumContextWindow = value }
            fun minimumMaxOutputTokens(value: Long?) = apply { minimumMaxOutputTokens = value }
            fun includeDeprecated(value: Boolean) = apply { includeDeprecated = value }
            fun featuredProviderIDs(value: List<String>) = apply { featuredProviderIDs = value }

            fun build() = ModelFilter(
                providerID = providerID,
                query = query,
                reasoning = reasoning,
                toolCalling = toolCalling,
                structuredOutput = structuredOutput,
                vision = vision,
                attachments = attachments,
                openWeights = openWeights,
                minimumContextWindow = minimumContextWindow,
                minimumMaxOutputTokens = minimumMaxOutputTokens,
                includeDeprecated = includeDeprecated,
                featuredProviderIDs = featuredProviderIDs,
            )
        }

        companion object {
            fun builder() = Builder()
        }
    }
}
