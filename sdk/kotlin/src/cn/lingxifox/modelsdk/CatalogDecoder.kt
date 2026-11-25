package cn.lingxifox.modelsdk

import cn.lingxifox.modelsdk.CatalogTypes.CatalogModel
import cn.lingxifox.modelsdk.CatalogTypes.CatalogProvider
import cn.lingxifox.modelsdk.CatalogTypes.CatalogRevision
import cn.lingxifox.modelsdk.CatalogTypes.ModelCapabilities
import cn.lingxifox.modelsdk.CatalogTypes.ModelLimits
import cn.lingxifox.modelsdk.CatalogTypes.ModelPricing
import cn.lingxifox.modelsdk.CatalogTypes.ModelStatus
import java.nio.charset.StandardCharsets
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneOffset
import java.util.Locale

/** Decodes `models.json` (schema 1.x/2.x) into typed catalog objects. */
object CatalogDecoder {
    @Suppress("UNCHECKED_CAST")
    private fun asObject(value: Any?): Map<String, Any?>? = value as? Map<String, Any?>

    private fun optString(value: Any?): String? = value as? String

    private fun nonEmpty(value: Any?): String? {
        val text = value as? String ?: return null
        return if (text.trim().isEmpty()) null else text
    }

    private fun optLong(value: Any?): Long? = when (value) {
        is Long -> value
        is Int -> value.toLong()
        is Double -> if (Math.rint(value) == value) value.toLong() else null
        else -> null
    }

    private fun optDouble(value: Any?): Double? = (value as? Number)?.toDouble()

    private fun optBool(value: Any?): Boolean? = value as? Boolean

    private fun stringList(value: Any?): List<String> {
        val list = value as? List<*> ?: return emptyList()
        return list.filterIsInstance<String>()
    }

    private fun hasPrefix(list: List<String>, prefix: String): Boolean =
        list.any { it.lowercase(Locale.ROOT).startsWith(prefix) }

    private fun parseTimestamp(text: String?): Instant? {
        if (text.isNullOrEmpty()) return null
        return try {
            Instant.parse(text)
        } catch (_: Exception) {
            try {
                LocalDate.parse(text).atStartOfDay(ZoneOffset.UTC).toInstant()
            } catch (_: Exception) {
                null
            }
        }
    }

    private fun decodeModel(providerID: String, key: String, fields: Map<String, Any?>): CatalogModel? {
        val id = nonEmpty(key) ?: nonEmpty(fields["id"]) ?: return null

        val limits = asObject(fields["limit"])
        val cost = asObject(fields["cost"])
        val modalities = asObject(fields["modalities"])

        val inputModalities = modalities?.let { stringList(it["input"]) } ?: emptyList()
        val outputModalities = modalities?.let { stringList(it["output"]) } ?: emptyList()
        val attachment = fields["attachment"] == true
        val vision = attachment || hasPrefix(inputModalities, "image")

        val efforts = ArrayList<String>()
        (fields["reasoning_options"] as? List<*>)?.forEach { option ->
            val om = asObject(option)
            if (om != null) efforts.addAll(stringList(om["values"]))
        }

        return CatalogModel(
            id = id,
            providerID = providerID,
            name = optString(fields["name"]) ?: id,
            family = nonEmpty(fields["family"]),
            overview = nonEmpty(fields["description"]),
            releaseDate = nonEmpty(fields["release_date"]),
            lastUpdated = nonEmpty(fields["last_updated"]),
            knowledgeCutoff = nonEmpty(fields["knowledge"]),
            canonicalModelID = nonEmpty(fields["canonical_model_id"]),
            status = ModelStatus.fromRaw(optString(fields["status"])),
            limits = ModelLimits(
                context = limits?.let { optLong(it["context"]) },
                input = limits?.let { optLong(it["input"]) },
                output = limits?.let { optLong(it["output"]) },
            ),
            capabilities = ModelCapabilities(
                reasoning = fields["reasoning"] == true,
                reasoningEfforts = efforts.toList(),
                toolCalling = fields["tool_call"] == true,
                structuredOutput = fields["structured_output"] == true,
                attachments = attachment,
                vision = vision,
                audioInput = hasPrefix(inputModalities, "audio"),
                audioOutput = hasPrefix(outputModalities, "audio"),
                inputModalities = inputModalities,
                outputModalities = outputModalities,
                openWeights = fields["open_weights"] == true,
                supportsTemperature = optBool(fields["temperature"]),
            ),
            pricing = ModelPricing(
                input = cost?.let { optDouble(it["input"]) },
                output = cost?.let { optDouble(it["output"]) },
                cacheRead = cost?.let { optDouble(it["cache_read"]) },
                cacheWrite = cost?.let { optDouble(it["cache_write"]) },
                reasoning = cost?.let { optDouble(it["reasoning"]) },
                inputAudio = cost?.let { optDouble(it["input_audio"]) },
                outputAudio = cost?.let { optDouble(it["output_audio"]) },
            ),
            fields = fields,
        )
    }

    private fun decodeProvider(key: String, fields: Map<String, Any?>): CatalogProvider? {
        val rawModels = asObject(fields["models"])
        val models = ArrayList<CatalogModel>()
        rawModels?.forEach { (modelKey, rawModel) ->
            val mf = asObject(rawModel) ?: return@forEach
            decodeModel(key, modelKey, mf)?.let { models.add(it) }
        }
        if (models.isEmpty() && !fields.containsKey("id")) return null
        val id = optString(fields["id"]) ?: key
        return CatalogProvider(
            id = id,
            name = optString(fields["name"]) ?: id,
            baseURL = nonEmpty(fields["baseURL"]),
            documentationURL = nonEmpty(fields["doc"]),
            environmentVariableNames = stringList(fields["env"]),
            models = CatalogOrdering.modelsInStableOrder(models),
            fields = fields,
        )
    }

    fun decode(data: ByteArray): LingXiModelCatalog {
        val parsed = try {
            Json.parse(String(data, StandardCharsets.UTF_8))
        } catch (e: Exception) {
            throw CatalogException(CatalogException.Kind.INVALID_CATALOG, "Invalid JSON: ${e.message}", 0, null, e)
        }
        val root = asObject(parsed)
            ?: throw CatalogException(CatalogException.Kind.INVALID_CATALOG, "Catalog root is not an object")
        val providersObj = asObject(root["providers"])
        if (providersObj.isNullOrEmpty()) {
            throw CatalogException(CatalogException.Kind.INVALID_CATALOG, "providers missing or empty")
        }

        var schemaVersion = optString(root["schemaVersion"]) ?: optString(root["version"]) ?: "1.0"
        val major = schemaVersion.split(".", limit = 2)[0]
        if (major != "1" && major != "2") {
            throw CatalogException(
                CatalogException.Kind.UNSUPPORTED_SCHEMA_VERSION,
                "Unsupported schema version: $schemaVersion",
                0,
                schemaVersion,
                null,
            )
        }

        val providers = ArrayList<CatalogProvider>()
        providersObj.forEach { (key, rawProvider) ->
            val pf = asObject(rawProvider) ?: return@forEach
            decodeProvider(key, pf)?.let { providers.add(it) }
        }
        if (providers.isEmpty()) {
            throw CatalogException(CatalogException.Kind.INVALID_CATALOG, "No valid provider entries")
        }

        val sorted = CatalogOrdering.sortProviders(providers, emptyList())
        val generatedRaw = optString(root["generatedAt"]) ?: optString(root["updatedAt"])
        val generatedAt = parseTimestamp(generatedRaw)

        var catalogRevision = optString(root["catalogRevision"]) ?: optString(root["catalogHash"])
        if (catalogRevision == null && generatedAt != null) catalogRevision = generatedAt.toString()
        if (catalogRevision == null) catalogRevision = ""

        val totalProviders = optLong(root["totalProviders"])?.toInt() ?: sorted.size
        val totalModels = optLong(root["totalModels"])?.toInt() ?: sorted.sumOf { it.modelCount }

        val revision = CatalogRevision(
            schemaVersion = schemaVersion,
            catalogRevision = catalogRevision,
            catalogHash = optString(root["catalogHash"]),
            source = optString(root["source"]),
            sourceURL = optString(root["sourceURL"]),
            sourceHash = optString(root["sourceHash"]),
            generatedAt = generatedAt,
            sourceFetchedAt = parseTimestamp(optString(root["sourceFetchedAt"])),
            totalProviders = totalProviders,
            totalModels = totalModels,
        )
        return LingXiModelCatalog(revision, sorted, data.copyOf())
    }
}
