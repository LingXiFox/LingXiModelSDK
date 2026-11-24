# Performance and Security Audit

## Host measurements

Measured with `./scripts/benchmark.sh` on Apple M5, macOS arm64, on 2026-11-24. Go benchmarks use `-benchmem`. The Gateway benchmark runs parallel requests through the in-process handler to a loopback HTTP upstream.

| Target | Time | Allocations or size |
| --- | ---: | ---: |
| Go Native client initialization | 12.66 ns/op | 56 B/op, 2 allocs/op |
| Go Unified request translation and response normalization | 342.6 ns/op | 1,464 B/op, 17 allocs/op |
| Go Native short SSE fixture parsing | 3.48 µs/fixture, 98.10 MB/s | 66,304 B/op, 10 allocs/op |
| Go Gateway parallel completions | 23.29 µs/op wall time per parallel iteration | 26,718 B/op, 179 allocs/op |
| Node.js 26 TypeScript translation and normalization | 0.37 µs/op over 10,000 iterations | Process memory unmeasured |
| Python to Rust C ABI translation and normalization | 45.83 µs/op over 5,000 iterations | Process memory unmeasured |
| macOS arm64 standalone website binary | — | 9,498,370 bytes |

Allocation measurements are per operation. Process RSS, long-duration memory and descriptor use, production TLS latency and target-host startup remain unmeasured.

## Security controls and evidence

| Area | Implementation and verification |
| --- | --- |
| Public catalog isolation | The public handler exposes catalog and documentation routes. Tests send an inference request and a credential header to public routes and verify rejection. The admin listener is separate. |
| Gateway SSRF | Remote bind requires authentication and a host allowlist. The Gateway rejects credential-bearing or query-bearing base URLs and checks resolved IPs before dialing. Loopback egress is limited to development listeners bound to loopback. |
| Redirects and TLS | Default HTTP clients block redirects for inference. HTTPS is the default for remote targets. Reviewed custom transports and platform SDK policy independently from the core coverage tests. |
| Inference errors | Gateway strips upstream bodies from HTTP and SSE error messages. A mock that echoes the Authorization header verifies the credential stays out of the Gateway response. Native SDK errors may still contain raw provider data by design. |
| Request and response bounds | Gateway requests are limited to 16 MiB. Rust and Go enforce 32 MiB on core inference responses and streams. Bedrock validates frame lengths and CRCs. Size and timeout parity across the remaining SDKs is open. |
| Stream lifecycle | Go closes response bodies and propagates client cancellation. The Gateway integration test reads data before the upstream completes, cancels the client, and observes upstream cancellation. TypeScript cancels its reader on early exit. The blocking Rust C ABI returns buffered events. |
| Supply chain | GitHub Actions use pinned refs and read-only repository permissions. Kotlin's existing compiler archive is SHA-256 checked. The release workflow builds Rust with the committed lockfile. An external vulnerability scan and end-to-end dependency provenance are open. |
