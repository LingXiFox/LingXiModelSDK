# LingXi Models website

Go 1.23+, standard library only. Embedded templates, CSS and JavaScript; no Node, Python, CDN assets or external runtime required. Model data remains mutable and persistent outside the executable.

```sh
cd website
go test ./...
CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o lingxi-models .
./lingxi-models -listen 127.0.0.1:8080 -data-dir ./data
```

For offline startup, copy a valid legacy models.json into the data directory first. On first startup without a snapshot the page and API report 503 until synchronization succeeds. To refresh once: `./lingxi-models -data-dir ./data -sync-once`. Use a single writer per data directory; do not run sync-once alongside the scheduled service. The default refresh is hourly; set -sync-interval 0 to disable it. The previous snapshot remains available on upstream failures or a >30% shrink. The catalog is persisted with atomic rename; /publication.json is generated from the same immutable snapshot, so it need not be stored as a separate mutable file.

Endpoints: /, /models.json, /publication.json, /healthz. JSON schema 2.0, ETag, HEAD, conditional GET, CORS and existing URL contract retained. UI uses server-side filters and bounded pagination, with small local JS for sidebar search, copying and SDK dialog. Unknown metadata fields remain in raw JSON. Provider protocol support is not inferred.

Compatibility notes: the Go canonical JSON serialization may cause a one-time revision change from the Python publisher. JSON object key order is not a contract; the website explicitly sorts the display. The legacy attachment flag is displayed separately from explicit image input. The UI is intentionally restyled, and the old filing icon is not bundled yet; filing numbers and links are preserved.

Deployment: build a Linux binary for the actual host architecture, seed the data directory from the existing live catalog, run on a loopback port via systemd, verify /healthz and compare /models.json before switching Caddy's models host to reverse_proxy. Preserve existing TLS/ESA and other virtual hosts. Do not overwrite the entire existing Caddyfile. No production deployment has been performed.
