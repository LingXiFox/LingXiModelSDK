# Performance Benchmarks & Security Audit (2.0.0-alpha.1)

## 1. Real Host Performance Benchmarks

Measured via `./scripts/benchmark.sh` on `Darwin 27.3.0 arm64` (Apple M5) at `2026-11-24T14:20:44Z`:

| Benchmark Target | Measurement | Throughput / Latency | Memory / Size |
| --- | --- | --- | --- |
| Go `unified.TranslateRequest` + `NormalizeResponse` (`go test -bench=. -benchmem`) | 2,667,326 iterations | **443.8 ns/op** | 1,464 B/op, 17 allocs/op |
| TypeScript LMP `translateRequest` + `normalizeResponse` (Node v26.13.1) | 10,000 iterations | **0.27 µs/op** (2.72 ms total) | Native ESM + TS type stripping |
| Python -> Rust `liblingxi_core` C ABI (`ctypes`) | 5,000 iterations | **47.67 µs/op** (238.37 ms total) | Shared Rust core (`cdylib`) |
| Rust `core` 200 sequential keep-alive requests (`agent_scenarios.rs`) | 200 HTTP calls | **< 40 ms** total test runtime | Single pooled `reqwest::Client` |
| Website (`models.lingxifox.cn`, `CGO_ENABLED=0`) | Static Go binary | — | **9.1 MB** (all HTML/CSS/JS/PNG embedded) |

## 2. Security Audit Checklist

| Area | Control Implemented | Verification |
| --- | --- | --- |
| **Catalog Isolation** | Public catalog site (`models.lingxifox.cn`) & Catalog SDKs never accept inference payloads or provider API keys. | Verified in `website/server.go` & Catalog SDKs. |
| **SSRF (Gateway Egress)** | `gateway/security.go` blocks loopback, RFC1918 (`10/8`, `172.16/12`, `192.168/16`), cloud metadata/link-local (`169.254/16`, `fe80::/10`), ULA (`fc00::/7`), CGNAT (`100.64/10`) both on URL validation and at TCP dial time via `NewSafeHTTPClient`. | `TestGatewaySSRFPrivateIPBlockAndEmbedRerank` in `gateway/gateway_test.go`. |
| **Fail-Closed Binding** | Non-loopback Gateway listen addresses require both `-auth-token` and `-allow-hosts`. | `ValidateStartupSecurity` in `gateway/security.go`. |
| **HTTP Redirect Blocking** | Automatic HTTP 3xx redirects are blocked across Rust (`Policy::none()`), Go (`http.ErrUseLastResponse`), TS (`redirect: "error"`), Swift (`URLSessionTaskDelegate`), Python (`_NoRedirectHandler`), Java (`Redirect.NEVER`), C# (`AllowAutoRedirect = false`), Kotlin (`Redirect.NEVER`), Dart (`followRedirects = false`) to prevent API key leakage. | Tested against 302 mock servers in Rust, Go, TS, Python, Java, C#, Kotlin, Dart. |
| **Payload & Response Caps** | Gateway limits incoming request bodies to 16 MiB; Rust & Go native clients cap upstream responses at 32 MiB. | `decodeBoundedJSON`, `maxNativeResponseBytes`, `MAX_RESPONSE_BYTES`. |
| **Secret Redaction** | `RedactSecret` masks credentials; API keys are never echoed in `/healthz` or catalog responses. | `TestGatewaySSRFPrivateIPBlockAndEmbedRerank`. |
| **Zero Unapproved Dependencies** | All 10 language SDKs use platform/stdlib crypto & HTTP (Rust uses only pre-approved `serde`, `serde_json`, `reqwest`, `tokio`); AWS SigV4 is implemented using stdlib/pure crypto. | Audited `Cargo.toml`, `go.mod`, `package.json`, `Package.swift`, `pom.xml`, `.csproj`. |
