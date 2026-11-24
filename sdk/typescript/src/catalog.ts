import { modelsInStableOrder, sortProviders } from "./ordering.ts";
import {
  isStatusSelectable,
  type CatalogModel,
  type CatalogProvider,
  type CatalogRevision,
  type ModelFilter,
} from "./types.ts";

function norm(text: string): string {
  return text.trim().toLowerCase();
}

export class LingXiModelCatalog {
  readonly revision: CatalogRevision;
  readonly providers: readonly CatalogProvider[];
  readonly rawDocument: Uint8Array | undefined;

  constructor(
    revision: CatalogRevision,
    providers: readonly CatalogProvider[],
    rawDocument?: Uint8Array
  ) {
    this.revision = revision;
    this.providers = providers;
    this.rawDocument = rawDocument;
  }

  get totalModels(): number {
    return this.providers.reduce((sum, p) => sum + p.models.length, 0);
  }

  provider(providerID: string): CatalogProvider | undefined {
    const key = norm(providerID);
    return this.providers.find((p) => norm(p.id) === key);
  }

  model(idOrProvider: string, modelID?: string): CatalogModel | undefined {
    if (modelID !== undefined) {
      const prov = this.provider(idOrProvider);
      if (!prov) return undefined;
      const key = norm(modelID);
      return prov.models.find((m) => norm(m.id) === key);
    }
    const slash = idOrProvider.indexOf("/");
    if (slash !== -1) {
      return this.model(idOrProvider.slice(0, slash), idOrProvider.slice(slash + 1));
    }
    const key = norm(idOrProvider);
    for (const prov of this.providers) {
      const match = prov.models.find((m) => norm(m.id) === key);
      if (match) return match;
    }
    return undefined;
  }

  sortedProviders(featured: readonly string[] = []): CatalogProvider[] {
    return sortProviders(this.providers, featured);
  }

  modelsForProvider(providerID: string): CatalogModel[] {
    const prov = this.provider(providerID);
    return prov ? modelsInStableOrder(prov.models) : [];
  }

  get allModels(): CatalogModel[] {
    return this.sortedProviders().flatMap((p) => modelsInStableOrder(p.models));
  }

  search(query: string, limit?: number): CatalogModel[] {
    const needle = norm(query);
    if (!needle) {
      const all = this.allModels;
      return limit !== undefined ? all.slice(0, limit) : all;
    }
    const matches: CatalogModel[] = [];
    for (const prov of this.sortedProviders()) {
      const ordered = modelsInStableOrder(prov.models);
      if (norm(prov.id).includes(needle) || norm(prov.name).includes(needle)) {
        matches.push(...ordered);
      } else {
        for (const m of ordered) {
          if (
            [m.id, m.name, m.family ?? "", m.overview ?? ""].some((v) =>
              norm(v).includes(needle)
            )
          ) {
            matches.push(m);
          }
        }
      }
      if (limit !== undefined && matches.length >= limit) {
        return matches.slice(0, limit);
      }
    }
    return limit !== undefined ? matches.slice(0, limit) : matches;
  }

  modelsMatching(filter: ModelFilter): CatalogModel[] {
    const matches: CatalogModel[] = [];
    for (const prov of this.sortedProviders(filter.featuredProviderIDs ?? [])) {
      if (filter.providerID !== undefined && norm(prov.id) !== norm(filter.providerID)) {
        continue;
      }
      for (const m of modelsInStableOrder(prov.models)) {
        if (filterMatches(m, prov, filter)) {
          matches.push(m);
        }
      }
    }
    return matches;
  }
}

function filterMatches(
  model: CatalogModel,
  provider: CatalogProvider,
  filter: ModelFilter
): boolean {
  if (!filter.includeDeprecated && !isStatusSelectable(model.status)) return false;
  if (filter.reasoning !== undefined && model.capabilities.reasoning !== filter.reasoning) {
    return false;
  }
  if (filter.toolCalling !== undefined && model.capabilities.toolCalling !== filter.toolCalling) {
    return false;
  }
  if (
    filter.structuredOutput !== undefined &&
    model.capabilities.structuredOutput !== filter.structuredOutput
  ) {
    return false;
  }
  if (filter.vision !== undefined && model.capabilities.vision !== filter.vision) return false;
  if (filter.attachments !== undefined && model.capabilities.attachments !== filter.attachments) {
    return false;
  }
  if (filter.openWeights !== undefined && model.capabilities.openWeights !== filter.openWeights) {
    return false;
  }
  if (
    filter.minimumContextWindow !== undefined &&
    (model.limits.context ?? 0) < filter.minimumContextWindow
  ) {
    return false;
  }
  if (
    filter.minimumMaxOutputTokens !== undefined &&
    (model.limits.output ?? 0) < filter.minimumMaxOutputTokens
  ) {
    return false;
  }
  if (filter.query !== undefined) {
    const q = norm(filter.query);
    if (
      q &&
      ![
        model.id,
        model.name,
        model.family ?? "",
        model.overview ?? "",
        provider.id,
        provider.name,
      ].some((v) => norm(v).includes(q))
    ) {
      return false;
    }
  }
  return true;
}
