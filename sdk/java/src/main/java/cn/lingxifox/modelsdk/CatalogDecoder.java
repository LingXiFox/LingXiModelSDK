package cn.lingxifox.modelsdk;

import cn.lingxifox.modelsdk.CatalogTypes.*;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.time.LocalDate;
import java.time.ZoneOffset;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Locale;
import java.util.Map;

public final class CatalogDecoder {
    private CatalogDecoder() {}

    @SuppressWarnings("unchecked")
    private static Map<String, Object> asObject(Object v) {
        return v instanceof Map<?, ?> m ? (Map<String, Object>) m : null;
    }

    private static String optString(Object v) {
        return v instanceof String s ? s : null;
    }

    private static String nonEmpty(Object v) {
        if (!(v instanceof String s)) return null;
        return s.trim().isEmpty() ? null : s;
    }

    private static Long optLong(Object v) {
        if (v instanceof Long l) return l;
        if (v instanceof Integer i) return i.longValue();
        if (v instanceof Double d && Math.rint(d) == d) return d.longValue();
        return null;
    }

    private static Double optDouble(Object v) {
        if (v instanceof Number n) return n.doubleValue();
        return null;
    }

    private static Boolean optBool(Object v) {
        return v instanceof Boolean b ? b : null;
    }

    private static List<String> stringList(Object v) {
        if (!(v instanceof List<?> list)) return List.of();
        List<String> out = new ArrayList<>();
        for (Object item : list) {
            if (item instanceof String s) out.add(s);
        }
        return List.copyOf(out);
    }

    private static boolean hasPrefix(List<String> list, String prefix) {
        for (String item : list) {
            if (item.toLowerCase(Locale.ROOT).startsWith(prefix)) return true;
        }
        return false;
    }

    private static Instant parseTimestamp(String text) {
        if (text == null || text.isEmpty()) return null;
        try {
            return Instant.parse(text);
        } catch (Exception ignored) {
            try {
                return LocalDate.parse(text).atStartOfDay(ZoneOffset.UTC).toInstant();
            } catch (Exception ignored2) {
                return null;
            }
        }
    }

    private static CatalogModel decodeModel(String providerID, String key, Map<String, Object> fields) {
        String id = nonEmpty(key);
        if (id == null) id = nonEmpty(fields.get("id"));
        if (id == null) return null;

        Map<String, Object> lim = asObject(fields.get("limit"));
        Map<String, Object> cost = asObject(fields.get("cost"));
        Map<String, Object> mod = asObject(fields.get("modalities"));

        List<String> inMods = mod != null ? stringList(mod.get("input")) : List.of();
        List<String> outMods = mod != null ? stringList(mod.get("output")) : List.of();
        boolean attachment = Boolean.TRUE.equals(fields.get("attachment"));
        boolean vision = attachment || hasPrefix(inMods, "image");

        List<String> efforts = new ArrayList<>();
        if (fields.get("reasoning_options") instanceof List<?> opts) {
            for (Object opt : opts) {
                Map<String, Object> om = asObject(opt);
                if (om != null) efforts.addAll(stringList(om.get("values")));
            }
        }

        return new CatalogModel(
            id,
            providerID,
            optString(fields.get("name")) != null ? optString(fields.get("name")) : id,
            nonEmpty(fields.get("family")),
            nonEmpty(fields.get("description")),
            nonEmpty(fields.get("release_date")),
            nonEmpty(fields.get("last_updated")),
            nonEmpty(fields.get("knowledge")),
            nonEmpty(fields.get("canonical_model_id")),
            ModelStatus.fromRaw(optString(fields.get("status"))),
            new ModelLimits(
                lim != null ? optLong(lim.get("context")) : null,
                lim != null ? optLong(lim.get("input")) : null,
                lim != null ? optLong(lim.get("output")) : null
            ),
            new ModelCapabilities(
                Boolean.TRUE.equals(fields.get("reasoning")),
                List.copyOf(efforts),
                Boolean.TRUE.equals(fields.get("tool_call")),
                Boolean.TRUE.equals(fields.get("structured_output")),
                attachment,
                vision,
                hasPrefix(inMods, "audio"),
                hasPrefix(outMods, "audio"),
                inMods,
                outMods,
                Boolean.TRUE.equals(fields.get("open_weights")),
                optBool(fields.get("temperature"))
            ),
            new ModelPricing(
                cost != null ? optDouble(cost.get("input")) : null,
                cost != null ? optDouble(cost.get("output")) : null,
                cost != null ? optDouble(cost.get("cache_read")) : null,
                cost != null ? optDouble(cost.get("cache_write")) : null,
                cost != null ? optDouble(cost.get("reasoning")) : null,
                cost != null ? optDouble(cost.get("input_audio")) : null,
                cost != null ? optDouble(cost.get("output_audio")) : null
            ),
            Collections.unmodifiableMap(fields)
        );
    }

    private static CatalogProvider decodeProvider(String key, Map<String, Object> fields) {
        Map<String, Object> rawModels = asObject(fields.get("models"));
        List<CatalogModel> models = new ArrayList<>();
        if (rawModels != null) {
            for (Map.Entry<String, Object> entry : rawModels.entrySet()) {
                Map<String, Object> mf = asObject(entry.getValue());
                if (mf == null) continue;
                CatalogModel m = decodeModel(key, entry.getKey(), mf);
                if (m != null) models.add(m);
            }
        }
        if (models.isEmpty() && !fields.containsKey("id")) {
            return null;
        }
        String id = optString(fields.get("id")) != null ? optString(fields.get("id")) : key;
        String name = optString(fields.get("name")) != null ? optString(fields.get("name")) : id;
        return new CatalogProvider(
            id,
            name,
            nonEmpty(fields.get("baseURL")),
            nonEmpty(fields.get("doc")),
            stringList(fields.get("env")),
            CatalogOrdering.modelsInStableOrder(models),
            Collections.unmodifiableMap(fields)
        );
    }

    public static LingXiModelCatalog decode(byte[] data) {
        Object parsed;
        try {
            parsed = JsonParser.parse(new String(data, StandardCharsets.UTF_8));
        } catch (Exception e) {
            throw new CatalogException(CatalogException.Kind.INVALID_CATALOG, "Invalid JSON: " + e.getMessage(), 0, null, e);
        }

        Map<String, Object> root = asObject(parsed);
        if (root == null) {
            throw new CatalogException(CatalogException.Kind.INVALID_CATALOG, "Catalog root is not an object");
        }
        Map<String, Object> providersObj = asObject(root.get("providers"));
        if (providersObj == null || providersObj.isEmpty()) {
            throw new CatalogException(CatalogException.Kind.INVALID_CATALOG, "providers missing or empty");
        }

        String schemaVersion = optString(root.get("schemaVersion"));
        if (schemaVersion == null) schemaVersion = optString(root.get("version"));
        if (schemaVersion == null) schemaVersion = "1.0";
        String major = schemaVersion.split("\\.", 2)[0];
        if (!"1".equals(major) && !"2".equals(major)) {
            throw new CatalogException(
                CatalogException.Kind.UNSUPPORTED_SCHEMA_VERSION,
                "Unsupported schema version: " + schemaVersion,
                0,
                schemaVersion,
                null
            );
        }

        List<CatalogProvider> providers = new ArrayList<>();
        for (Map.Entry<String, Object> entry : providersObj.entrySet()) {
            Map<String, Object> pf = asObject(entry.getValue());
            if (pf == null) continue;
            CatalogProvider p = decodeProvider(entry.getKey(), pf);
            if (p != null) providers.add(p);
        }
        if (providers.isEmpty()) {
            throw new CatalogException(CatalogException.Kind.INVALID_CATALOG, "No valid provider entries");
        }

        List<CatalogProvider> sorted = CatalogOrdering.sortProviders(providers, List.of());
        String genRaw = optString(root.get("generatedAt"));
        if (genRaw == null) genRaw = optString(root.get("updatedAt"));
        Instant generatedAt = parseTimestamp(genRaw);

        String catRev = optString(root.get("catalogRevision"));
        if (catRev == null) catRev = optString(root.get("catalogHash"));
        if (catRev == null && generatedAt != null) catRev = generatedAt.toString();
        if (catRev == null) catRev = "";

        Long tp = optLong(root.get("totalProviders"));
        int totalProviders = tp != null ? tp.intValue() : sorted.size();
        Long tm = optLong(root.get("totalModels"));
        int totalModels = tm != null ? tm.intValue() : sorted.stream().mapToInt(CatalogProvider::modelCount).sum();

        CatalogRevision rev = new CatalogRevision(
            schemaVersion,
            catRev,
            optString(root.get("catalogHash")),
            optString(root.get("source")),
            optString(root.get("sourceURL")),
            optString(root.get("sourceHash")),
            generatedAt,
            parseTimestamp(optString(root.get("sourceFetchedAt"))),
            totalProviders,
            totalModels
        );
        return new LingXiModelCatalog(rev, sorted, data.clone());
    }
}
