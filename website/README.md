# LingXi Models Website (`website/`)

Single-binary Go 1.23+ web server for `models.lingxifox.cn`. All HTML templates, CSS, JavaScript, and the filing icon (`static/beian-icon.png`) are embedded via `go:embed`. Zero external Go dependencies, zero Node.js/Python runtime dependencies, zero CDN assets.

## Build & Run

```sh
cd website
go vet ./...
go test -race ./...
CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o lingxi-models .
./lingxi-models -listen 127.0.0.1:8080 -data-dir ./data
```

Copy the compiled `lingxi-models` binary to an empty directory and run it — no external web assets are required.

## CLI Flags & Sync Modes

- `-listen` (default `127.0.0.1:8080`): public HTTP listen address.
- `-data-dir` (default `./data`): persistent directory for `models.json`, `publication.json`, and `publication.json.prev`.
- `-source` (default `https://models.dev/api.json`): upstream catalog URL.
- `-sync-interval` (default `1h`): background refresh interval (`0` disables scheduled sync).
- `-sync-once`: fetch, validate, atomically publish once, and exit.
- `-allow-shrink`: allow >30% model/provider count drop (valid only with `-sync-once`).
- `-min-model-ratio` (default `0.7`): shrink-guard threshold vs previous snapshot.
- `-check`: offline validation of `models.json` in `-data-dir` (prints manifest JSON and exits).
- `-admin-listen` / `-sync-token`: optional loopback-only admin HTTP server exposing `POST /admin/sync` (disabled by default; require `Authorization: Bearer <token>` when `-sync-token` is set).
- `SIGHUP` (Unix): triggers an immediate manual sync in the running daemon (`systemctl reload lingxi-models`).

## Endpoints

- `/` (and `/index.html` -> `301 /`): model catalog browser with server-side search, capability filters, provider details, and pagination.
- `/sdk`: Swift SDK docs & multi-language roadmap.
- `/protocols`: protocol support & verification roadmap.
- `/providers`: provider verification status directory (currently `Discovered`).
- `/models.json`: schema `2.0` catalog JSON (`ETag`, `Last-Modified`, `If-None-Match`, `If-Modified-Since`, `HEAD`, `OPTIONS`, CORS).
- `/publication.json`: publication manifest JSON derived from the same immutable snapshot.
- `/healthz`: JSON health diagnostics (`200` when snapshot loaded, `503` when unavailable).
- `/static/*` and `/beian-icon.png`: embedded static assets.

## Compatibility Notes vs Legacy `sync-models.py`

1. **Data semantics**: Both normalize `https://models.dev/api.json` into schema `2.0`, preserve all unknown provider and model fields, strip runtime guess keys (`swiftDriver`, `reasoningField`, `swiftSnippet`), derive `baseURL` from `api` (or `null`), compute `modelCount`, and reject >30% model/provider drops.
2. **Sorting**: Both emit deterministic provider and model ordering (`naturalCompare` matching Swift `ModelCatalogOrdering`: numbers sort before text, `GPT-5` before `GPT-10`, models sorted `release_date` descending with undated entries last).
3. **Canonical hashing**: Go recursively canonicalizes nested JSON objects via `json.Decoder` with `UseNumber()` and sorted keys. Because Python `json.dumps` and Go `encoding/json` differ slightly in float/Unicode escaping, `catalogRevision` and `catalogHash` may shift once on migration from Python-generated files while remaining deterministic across Go runs.
4. **Publication behavior**: Both persist `models.json` via temp-file write -> fsync -> close -> reopen & SHA-256/schema verification -> atomic rename, and maintain `publication.json` + `publication.json.prev`. In-memory serving reads `models.json` and `publication.json` from one immutable `Snapshot` so they can never drift out of sync.
