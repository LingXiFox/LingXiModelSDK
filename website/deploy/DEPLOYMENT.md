# Deployment Runbook (models.lingxifox.cn)

> Production changes require explicit approval. Do not overwrite the full live `/etc/caddy/Caddyfile`.

## 1. Build Linux Binary

```sh
cd website
CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -trimpath -ldflags="-s -w" -o lingxi-models .
```

Use `GOARCH=arm64` for ARM64 hosts.

## 2. Seed Persistent Data & Install Service

1. Copy the existing live `/var/www/models.lingxifox.cn/models.json` (and `publication.json` if present) into `/var/lib/lingxi-models/` so startup has immediate last-known-good data even if upstream is temporarily unreachable.
2. Validate the seeded snapshot offline:
   ```sh
   /usr/local/bin/lingxi-models -data-dir /var/lib/lingxi-models -check
   ```
3. Install `website/deploy/lingxi-models.service` to `/etc/systemd/system/lingxi-models.service`, then enable and start:
   ```sh
   sudo systemctl daemon-reload
   sudo systemctl enable --now lingxi-models
   ```

## 3. Smoke Test Before Caddy Switch

```sh
curl -fsS http://127.0.0.1:18080/healthz
curl -fsSI http://127.0.0.1:18080/models.json
curl -fsS http://127.0.0.1:18080/publication.json
```

Compare `/models.json` and `/publication.json` against the existing static files before touching Caddy.

## 4. Caddy Hot Reload (Approved Switch Only)

1. Back up `/etc/caddy/Caddyfile`.
2. Update **only** the `http://models.lingxifox.cn` and `models.lingxifox.cn` blocks using `website/deploy/caddy-models-snippet.caddy` (preserving all other virtual hosts, `admin unix//run/caddy/admin.sock|0600`, ZeroSSL, and `lingxi-esa-origin-ca.pem`).
3. Validate and hot-reload via `caddy validate` + `systemctl reload caddy` (or existing `deploy-caddy.sh`).

## 5. Rollback

Restore the backed-up `Caddyfile` (pointing `models.lingxifox.cn` back to `/var/www/models.lingxifox.cn`) and run `systemctl reload caddy`.
