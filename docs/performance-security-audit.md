# Performance and Security Audit

## Host measurements

Measured with `./scripts/benchmark.sh` on Apple M5, macOS arm64, on 2026-11-25 at 15:10 UTC (candidate `c717c3e`). Go benchmarks use `-benchmem`. The Gateway benchmark runs parallel requests through the in-process handler to a loopback HTTP upstream.

| Target | Time | Allocations or size |
| --- | ---: | ---: |
| Go Native client initialization | 12.92 ns/op | 56 B/op, 2 allocs/op |
| Go Unified request translation and response normalization | 421.5 ns/op | 1,480 B/op, 19 allocs/op |
| Go Native short SSE fixture parsing | 3.32 µs/fixture, 102.68 MB/s | 66,304 B/op, 10 allocs/op |
| Go Gateway parallel completions | 25.33 µs/op from aggregate wall time | 28,092 B/op, 182 allocs/op |
| Node.js 26 TypeScript translation and normalization | 0.51 µs/op over 10,000 iterations | Process memory unmeasured |
| Python to Rust C ABI translation and normalization | 47.68 µs/op over 5,000 iterations | Process memory unmeasured |
| Python to Rust C ABI incremental frame normalization | 13.65 µs/frame over 30,000 frames | Process memory unmeasured |
| macOS arm64 standalone website binary | — | 9,498,386 bytes |

A separate 60-second, 16-worker loopback load check completed 589,357 requests through a standalone Gateway process. The upstream observed 15 TCP connections. Gateway RSS moved from 11,232 KiB at startup to 23,024 KiB immediately after the run. The process had 10 open entries at startup and 25 after the run, while pooled connections were still warm.

## Long-duration soak

`./scripts/soak.sh 1800` ran on 2026-11-25 for 1,801 seconds against `conformance/soak/soak_server.py`, a loopback upstream that answers every native wire path with paced, protocol-correct frames. Each round drove all nine protocols through a complete request, an incremental stream and a mid-stream cancellation, plus one OpenAI embedding and one Cohere rerank. The driver sampled its own RSS and open descriptors every ten rounds and failed on any unexpected error.

| Metric | Result |
| --- | ---: |
| Rounds | 875 |
| Completions / streams / cancels | 7,875 each |
| Embeddings / reranks | 875 each |
| Failures, including malformed or truncated streams | 0 |
| Upstream requests observed | 25,375 |
| Upstream TCP connections accepted | 257 |
| Driver RSS | 7,872 KiB → 7,968 KiB (+96 KiB) |
| Driver open descriptors | 8 → 8 |

The driver reused connections for sequential requests, so 25,375 requests for 257 accepted connections is the expected reuse pattern rather than a leak. RSS stayed inside a 128 KiB band across the run and descriptors never moved, which rules out the slow accumulation that a buffered or handle-leaking stream path would show.

Allocation measurements are per operation. Idle-pool decay, production TLS latency, Linux-host RSS and target-host startup remain unmeasured.

## Security controls and evidence

| Area | Implementation and verification |
| --- | --- |
| Public catalog isolation | The public handler exposes catalog and documentation routes. Tests send an inference request and a credential header to public routes and verify rejection. The admin listener is separate. |
| Gateway SSRF | Remote bind requires authentication and a host allowlist. The Gateway rejects credential-bearing or query-bearing base URLs and checks resolved IPs before dialing. Loopback egress is limited to development listeners bound to loopback. |
| Redirects and TLS | Default HTTP clients block redirects for inference. HTTPS is the default for remote targets. Reviewed custom transports and platform SDK policy independently from the core coverage tests. |
| Inference errors | Gateway strips upstream bodies from HTTP and SSE error messages. A mock that echoes the Authorization header verifies the credential stays out of the Gateway response. Native SDK errors may still contain raw provider data by design. |
| Request and response bounds | Gateway requests are limited to 16 MiB. Rust and Go enforce 32 MiB on core inference responses and streams; the C ABI stream handles inherit the same ceiling. Bedrock validates frame lengths and CRCs. Language bindings that own their own HTTP transport enforce the same 32 MiB body limit. |
| Stream lifecycle | Go closes response bodies and propagates client cancellation. The Gateway integration test reads data before the upstream completes, cancels the client, and observes upstream cancellation. TypeScript cancels its reader on early exit. The Rust core delivers events incrementally through `EventStreamHandle`/`LmpStreamHandle`, and the C ABI exposes `lingxi_stream_open/next/cancel/close` with registry-checked handles; `core/tests/stream_incremental.rs` covers early exit, blocked-read cancellation, idle timeout, truncated streams and handle reuse after close. |
| Supply chain | GitHub Actions use pinned refs and read-only repository permissions. Kotlin's existing compiler archive is SHA-256 checked. The release workflow builds Rust with the committed lockfile. Trivy 0.76.2 reported zero findings in the detected manifests (two Cargo lockfiles, three Go modules, and Maven) and in a separate secret scan of tracked files. This scan covers the detected source manifests and tracked content. |