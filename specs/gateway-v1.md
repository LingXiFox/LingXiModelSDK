# LingXi Optional HTTP Gateway Specification (v1.0)

Status: Phase 5.

## 1. Architectural Boundary

- The HTTP Gateway is an **optional**, self-hosted subsystem wrapping the Unified LMP & Native Registry layers.
- Model Catalog (`models.lingxifox.cn`), Native Provider SDKs, and Unified in-process SDKs NEVER depend on this Gateway.
- Public `models.lingxifox.cn` MUST NEVER accept provider API keys or inference requests.

## 2. Security Defaults

- **Loopback by default**: `-listen` defaults to `127.0.0.1:18090`.
- **Fail-closed remote deployment**: Non-loopback bind MUST be refused at startup unless BOTH `-auth-token` (Bearer auth) AND `-allow-hosts` (egress allowlist) are configured.
- **SSRF protection**: Outbound targets (from registry or `endpointOverride`) are checked against `-allow-hosts` before any connection is opened.
- **No credential/body logging**: `X-Provider-API-Key` and request/response payloads are never logged.
- **Bounded requests & rate limiting**: Request bodies capped at 16 MiB; per-client token bucket rate limiter returns `429 Too Many Requests`.

## 3. Versioned HTTP Endpoints

- `GET /v1/healthz`: Gateway status, registry provider count, listen mode (`loopback`/`remote`).
- `POST /v1/lmp/complete`: Synchronous normalized LMP completion.
- `POST /v1/lmp/stream`: Server-Sent Events (`text/event-stream`) streaming normalized LMP events, propagating client cancellation via `context.Context`.
- `POST /v1/diagnostics/connect`: Validates registry entry, protocol declaration, and egress allowlist without sending inference prompts.
