package cn.lingxifox.modelsdk;

import cn.lingxifox.modelsdk.CatalogTypes.*;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Optional;

public final class LingXiModelCatalog {
    private final CatalogRevision revision;
    private final List<CatalogProvider> providers;
    private final byte[] rawDocument;

    public LingXiModelCatalog(CatalogRevision revision, List<CatalogProvider> providers, byte[] rawDocument) {
        this.revision = revision;
        this.providers = List.copyOf(providers);
        this.rawDocument = rawDocument;
    }

    public static LingXiModelCatalog decode(byte[] data) {
        return CatalogDecoder.decode(data);
    }

    public CatalogRevision revision() { return revision; }
    public List<CatalogProvider> providers() { return providers; }
    public byte[] rawDocument() { return rawDocument != null ? rawDocument.clone() : null; }

    public int totalModels() {
        return providers.stream().mapToInt(CatalogProvider::modelCount).sum();
    }

    private static String norm(String text) {
        return text == null ? "" : text.trim().toLowerCase(Locale.ROOT);
    }

    public Optional<CatalogProvider> provider(String providerID) {
        String key = norm(providerID);
        return providers.stream().filter(p -> norm(p.id()).equals(key)).findFirst();
    }

    public Optional<CatalogModel> model(String providerID, String modelID) {
        Optional<CatalogProvider> prov = provider(providerID);
        if (prov.isEmpty()) return Optional.empty();
        String key = norm(modelID);
        return prov.get().models().stream().filter(m -> norm(m.id()).equals(key)).findFirst();
    }

    public Optional<CatalogModel> model(String id) {
        int slash = id.indexOf('/');
        if (slash >= 0) {
            return model(id.substring(0, slash), id.substring(slash + 1));
        }
        String key = norm(id);
        for (CatalogProvider prov : providers) {
            for (CatalogModel m : prov.models()) {
                if (norm(m.id()).equals(key)) return Optional.of(m);
            }
        }
        return Optional.empty();
    }

    public List<CatalogProvider> sortedProviders(List<String> featured) {
        return CatalogOrdering.sortProviders(providers, featured);
    }

    public List<CatalogModel> modelsForProvider(String providerID) {
        return provider(providerID).map(p -> CatalogOrdering.modelsInStableOrder(p.models())).orElse(List.of());
    }

    public List<CatalogModel> allModels() {
        List<CatalogModel> out = new ArrayList<>();
        for (CatalogProvider p : sortedProviders(List.of())) {
            out.addAll(CatalogOrdering.modelsInStableOrder(p.models()));
        }
        return List.copyOf(out);
    }

    public List<CatalogModel> search(String query, Integer limit) {
        String needle = norm(query);
        if (needle.isEmpty()) {
            List<CatalogModel> all = allModels();
            if (limit != null && all.size() > limit) return all.subList(0, limit);
            return all;
        }
        List<CatalogModel> matches = new ArrayList<>();
        for (CatalogProvider p : sortedProviders(List.of())) {
            List<CatalogModel> ordered = CatalogOrdering.modelsInStableOrder(p.models());
            if (norm(p.id()).contains(needle) || norm(p.name()).contains(needle)) {
                matches.addAll(ordered);
            } else {
                for (CatalogModel m : ordered) {
                    if (containsAny(needle, m.id(), m.name(), m.family(), m.overview())) {
                        matches.add(m);
                    }
                }
            }
            if (limit != null && matches.size() >= limit) {
                return List.copyOf(matches.subList(0, limit));
            }
        }
        return List.copyOf(matches);
    }

    public List<CatalogModel> modelsMatching(ModelFilter filter) {
        List<CatalogModel> matches = new ArrayList<>();
        for (CatalogProvider p : sortedProviders(filter.featuredProviderIDs())) {
            if (filter.providerID() != null && !norm(p.id()).equals(norm(filter.providerID()))) {
                continue;
            }
            for (CatalogModel m : CatalogOrdering.modelsInStableOrder(p.models())) {
                if (matchesFilter(m, p, filter)) matches.add(m);
            }
        }
        return List.copyOf(matches);
    }

    private static boolean matchesFilter(CatalogModel m, CatalogProvider p, ModelFilter f) {
        if (!f.includeDeprecated() && !m.status().isSelectable()) return false;
        if (f.reasoning() != null && m.capabilities().reasoning() != f.reasoning()) return false;
        if (f.toolCalling() != null && m.capabilities().toolCalling() != f.toolCalling()) return false;
        if (f.structuredOutput() != null && m.capabilities().structuredOutput() != f.structuredOutput()) return false;
        if (f.vision() != null && m.capabilities().vision() != f.vision()) return false;
        if (f.attachments() != null && m.capabilities().attachments() != f.attachments()) return false;
        if (f.openWeights() != null && m.capabilities().openWeights() != f.openWeights()) return false;
        if (f.minimumContextWindow() != null) {
            long ctx = m.limits().context() != null ? m.limits().context() : 0L;
            if (ctx < f.minimumContextWindow()) return false;
        }
        if (f.minimumMaxOutputTokens() != null) {
            long out = m.limits().output() != null ? m.limits().output() : 0L;
            if (out < f.minimumMaxOutputTokens()) return false;
        }
        if (f.query() != null) {
            String q = norm(f.query());
            if (!q.isEmpty() && !containsAny(q, m.id(), m.name(), m.family(), m.overview(), p.id(), p.name())) {
                return false;
            }
        }
        return true;
    }

    private static boolean containsAny(String needle, String... fields) {
        for (String f : fields) {
            if (f != null && norm(f).contains(needle)) return true;
        }
        return false;
    }
}
