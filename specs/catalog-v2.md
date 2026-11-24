# LingXi Model Catalog Specification (v2.0)

Status: Frozen for Phase 2 multi-language Model Catalog SDKs.

## 1. Scope & Source

- Public endpoint: `https://models.lingxifox.cn/models.json`
- Publication manifest: `https://models.lingxifox.cn/publication.json`
- Supported document schema major versions: `1` (`"1.0"` / `version`) and `2` (`"2.0"` / `schemaVersion`). Any other major version MUST fail with `UnsupportedSchemaVersion`.
- Catalog SDKs are read-only metadata clients: zero inference dependencies, zero API keys, zero runtime protocol assumptions.

## 2. Three-State Value Semantics & Unknown Fields

- `null` or omitted fields MUST remain `null`/`nil`/`None`/`undefined` (unknown), NEVER coerced to `0` or `false`.
- Explicit `0` (e.g., `cost.input: 0`, `limit.context: 0`) MUST be preserved as zero.
- Explicit `false` (e.g., `temperature: false`, `structured_output: false`) MUST be preserved as `false`.
- All unmodeled provider and model JSON fields MUST be retained verbatim in `fields` / raw metadata.
- Disk caches MUST persist the raw published JSON bytes verbatim so unknown fields survive round-trips.

## 3. Decoding & Lenient Entry Handling

- Root document MUST be a JSON object with a non-empty `providers` object; otherwise raise `InvalidCatalog`.
- Individual malformed model entries (non-object, or both object key and `id` empty) are skipped without failing the catalog.
- Model identity: the JSON object key under `providers.<pid>.models` is the authoritative `id`; internal `model.id` is only a fallback when the key is empty.
- Capability derivation:
  - `vision`: `true` if `modalities.input` contains any string whose lowercased form starts with `"image"`, OR `attachment == true`.
  - `audioInput` / `audioOutput`: `true` if `modalities.input` / `modalities.output` contains any string whose lowercased form starts with `"audio"`.
  - `reasoningEfforts`: flattened `values` string arrays from `reasoning_options`.
  - `status`: `"deprecated"` / `"retired"` -> `deprecated` (`isSelectable = false`); `"beta"` / `"preview"` -> `preview` (`isSelectable = true`); `"active"` -> `active` (`isSelectable = true`); anything else -> `unknown` (`isSelectable = true`).

## 4. Deterministic Ordering (`ModelCatalogOrdering`)

- Tokenize strings into ASCII digit runs (`number`) and lowercased non-digit runs (`text`).
- Numbers ALWAYS sort before text at the same token position.
- Two numbers compare by numeric value (strip leading zeros -> shorter length first -> lexicographical), with raw digit string as tie-break.
- **Provider order**: `(featuredIndex, naturalCompare(name), id)`.
- **Model order**: `(hasReleaseDate ? 0 : 1, releaseDate DESC, naturalCompare(family), naturalCompare(name), id)`.

## 5. Lookup, Search & Filtering

- `provider(id)`: case-insensitive trimmed match on `provider.id`.
- `model(providerID, modelID)`: case-insensitive trimmed match on both IDs.
- `model(id)`: if `id` contains `/`, split on first `/` into `(providerID, modelID)`; otherwise search providers in publication order for a bare `model.id` match.
- `search(query, limit)`: empty query returns `allModels` (up to `limit`); otherwise case-insensitive substring match against `provider.id`/`provider.name` (includes all models of matching provider) or model `id`/`name`/`family`/`overview`.
- `models(matching: filter)`: excludes non-selectable (`deprecated`/`retired`) models unless `includeDeprecated` is `true`. Filters by `providerID`, `query` (including provider `id`/`name`), capability booleans, `minimumContextWindow`, `minimumMaxOutputTokens`, and `featuredProviderIDs`.

## 6. Network Caching & Offline Resilience

- Support memory cache + optional disk cache (`models.json` + `models.json.meta.json` storing `etag` and `fetchedAt`).
- When `!forceRefresh` and cached copy is fresh (`now - fetchedAt < maxAge`, with `maxAge > 0`), return cached copy without network I/O.
- Otherwise send `GET` with `If-None-Match` when an `ETag` is cached.
- On `304 Not Modified`, update `fetchedAt` and return cached copy.
- On network error, non-2xx HTTP status, or decode failure: if any cached copy exists, return the cached copy; otherwise raise an explicit error (`UnexpectedStatus`, `Transport`, or `Unavailable`).
- Offline `decode(bytes)` MUST work without network or cache.
