package cn.lingxifox.modelsdk;

import java.time.Instant;
import java.util.List;
import java.util.Map;

public final class CatalogTypes {
    private CatalogTypes() {}

    public enum ModelStatus {
        ACTIVE,
        PREVIEW,
        DEPRECATED,
        RETIRED,
        UNKNOWN;

        public static ModelStatus fromRaw(String raw) {
            if ("deprecated".equals(raw) || "retired".equals(raw)) return DEPRECATED;
            if ("beta".equals(raw) || "preview".equals(raw)) return PREVIEW;
            if ("active".equals(raw)) return ACTIVE;
            return UNKNOWN;
        }

        public boolean isSelectable() {
            return this != DEPRECATED && this != RETIRED;
        }
    }

    public record ModelLimits(Long context, Long input, Long output) {}

    public record ModelCapabilities(
        boolean reasoning,
        List<String> reasoningEfforts,
        boolean toolCalling,
        boolean structuredOutput,
        boolean attachments,
        boolean vision,
        boolean audioInput,
        boolean audioOutput,
        List<String> inputModalities,
        List<String> outputModalities,
        boolean openWeights,
        Boolean supportsTemperature
    ) {}

    public record ModelPricing(
        Double input,
        Double output,
        Double cacheRead,
        Double cacheWrite,
        Double reasoning,
        Double inputAudio,
        Double outputAudio
    ) {
        public boolean isUnknown() {
            return input == null
                && output == null
                && cacheRead == null
                && cacheWrite == null
                && reasoning == null
                && inputAudio == null
                && outputAudio == null;
        }
    }

    public record CatalogModel(
        String id,
        String providerID,
        String name,
        String family,
        String overview,
        String releaseDate,
        String lastUpdated,
        String knowledgeCutoff,
        String canonicalModelID,
        ModelStatus status,
        ModelLimits limits,
        ModelCapabilities capabilities,
        ModelPricing pricing,
        Map<String, Object> fields
    ) {
        public String qualifiedID() {
            return providerID + "/" + id;
        }

        public Long contextWindow() {
            return limits.context();
        }

        public Long maxOutputTokens() {
            return limits.output();
        }
    }

    public record CatalogProvider(
        String id,
        String name,
        String baseURL,
        String documentationURL,
        List<String> environmentVariableNames,
        List<CatalogModel> models,
        Map<String, Object> fields
    ) {
        public int modelCount() {
            return models.size();
        }
    }

    public record CatalogRevision(
        String schemaVersion,
        String catalogRevision,
        String catalogHash,
        String source,
        String sourceURL,
        String sourceHash,
        Instant generatedAt,
        Instant sourceFetchedAt,
        int totalProviders,
        int totalModels
    ) {}

    public record ModelFilter(
        String providerID,
        String query,
        Boolean reasoning,
        Boolean toolCalling,
        Boolean structuredOutput,
        Boolean vision,
        Boolean attachments,
        Boolean openWeights,
        Long minimumContextWindow,
        Long minimumMaxOutputTokens,
        boolean includeDeprecated,
        List<String> featuredProviderIDs
    ) {
        public static Builder builder() {
            return new Builder();
        }

        public static final class Builder {
            private String providerID;
            private String query;
            private Boolean reasoning;
            private Boolean toolCalling;
            private Boolean structuredOutput;
            private Boolean vision;
            private Boolean attachments;
            private Boolean openWeights;
            private Long minimumContextWindow;
            private Long minimumMaxOutputTokens;
            private boolean includeDeprecated;
            private List<String> featuredProviderIDs = List.of();

            public Builder providerID(String v) { this.providerID = v; return this; }
            public Builder query(String v) { this.query = v; return this; }
            public Builder reasoning(Boolean v) { this.reasoning = v; return this; }
            public Builder toolCalling(Boolean v) { this.toolCalling = v; return this; }
            public Builder structuredOutput(Boolean v) { this.structuredOutput = v; return this; }
            public Builder vision(Boolean v) { this.vision = v; return this; }
            public Builder attachments(Boolean v) { this.attachments = v; return this; }
            public Builder openWeights(Boolean v) { this.openWeights = v; return this; }
            public Builder minimumContextWindow(Long v) { this.minimumContextWindow = v; return this; }
            public Builder minimumMaxOutputTokens(Long v) { this.minimumMaxOutputTokens = v; return this; }
            public Builder includeDeprecated(boolean v) { this.includeDeprecated = v; return this; }
            public Builder featuredProviderIDs(List<String> v) { this.featuredProviderIDs = v; return this; }

            public ModelFilter build() {
                return new ModelFilter(
                    providerID, query, reasoning, toolCalling, structuredOutput,
                    vision, attachments, openWeights, minimumContextWindow,
                    minimumMaxOutputTokens, includeDeprecated, featuredProviderIDs
                );
            }
        }
    }
}
