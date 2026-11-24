# Phase 7 Audit

## Phase 1–6 Review

| Phase | Code, tests and edge cases reviewed | Result |
| --- | --- | --- |
| 1. Website | Embedded routes, conditional downloads, snapshot sync, filing links, deployment templates | Public POST isolation has an integration test. The provider page also includes curated providers missing from the current model snapshot. |
| 2. Catalog | Standalone package boundaries, catalog fixtures, ten language targets | The six original SDKs run catalog conformance. Newer C#, Kotlin, C++, and Dart tests cover a smaller surface. C++ still uses limited JSON lookup. |
| 3. Registry and Native | Curated endpoint and auth configuration, classifier, wire routing, native fixture runs | 327 registry entries checked. Discovery auth remains unknown until curated. Vertex and Azure require resource URLs. Bedrock streaming uses the binary AWS format and rejects malformed frames. |
| 4. Unified | Rust, Go and TS adapters, FFI transport, two-turn Agent wire checks | Shared Gemini fixture verifies parallel same-name calls, original thought signatures, cached content, zero usage, and inline media. Several protocol and language combinations still only expose text helpers. |
| 5. Gateway | Host checks and redirect policy, upstream status errors, streaming and request sizes | Gateway strips upstream error bodies and cancels upstream on client disconnect. Catalog and Gateway use separate listeners and build targets. |
| 6. Evidence and release | Generated reports, GitHub Actions job results, standalone fixtures and staging | The pinned workflows build candidates and retain commit and SHA-256 provenance. The live probe invokes the actual SDK and leaves registry status updates to recorded evidence. |

## Verification history

The initial Phase 7 commit `d017031` passed Website, Native Providers, Unified LMP and Gateway on GitHub Actions. Catalog SDKs and All 10 SDKs failed. The follow-up `c3425f9` passed both replacement runs.

- [Catalog SDKs recovery](https://github.com/LingXiFox/LingXiModelSDK/actions/runs/45413773034)
- [All 10 SDKs recovery and per-language jobs](https://github.com/LingXiFox/LingXiModelSDK/actions/runs/45413772901)

Candidate commit [`2d4a8fc`](https://github.com/LingXiFox/LingXiModelSDK/commit/2d4a8fc15faa0f15da3b4ece0f4cad5dccbd50e3) passed all six push workflows.

| Workflow | Candidate result |
| --- | --- |
| All 10 SDKs & Agent Conformance | [Passed](https://github.com/LingXiFox/LingXiModelSDK/actions/runs/45419159442) |
| Catalog SDKs | [Passed](https://github.com/LingXiFox/LingXiModelSDK/actions/runs/45419159919) |
| Native Providers | [Passed](https://github.com/LingXiFox/LingXiModelSDK/actions/runs/45419159886) |
| Unified LMP | [Passed](https://github.com/LingXiFox/LingXiModelSDK/actions/runs/45419159631) |
| Gateway | [Passed](https://github.com/LingXiFox/LingXiModelSDK/actions/runs/45419159654) |
| Website | [Passed](https://github.com/LingXiFox/LingXiModelSDK/actions/runs/45419159291) |

Local checks also exercised the built npm tarball from an installed package directory, executed both C++ runtime HTTP modes against a loopback fixture, and staged a separate website instance from a fixture snapshot. The nine credentialed probes reported nine skips with an empty environment. The probe tool also passed mock success and failure checks. The provider ledger remains unchanged by those tool checks.

The [candidate artifact](https://github.com/LingXiFox/LingXiModelSDK/actions/runs/45419259399/artifacts/14931774519) was downloaded and verified against `COMMIT.txt` and all 11 listed SHA-256 checksums. The Linux Go binaries are statically linked for amd64 and arm64. The Rust shared library targets Linux x86_64.

The reproducible local checks are `./scripts/test-all.sh` and `./scripts/benchmark.sh`. The local runner exercises Swift, Rust, Go, TypeScript, Python, Java, C++ and both Go applications. The all-SDK workflow also runs C#, Kotlin, and Dart. A passing build or smoke test provides evidence for the cases it exercises.

## Remaining production gates

- Complete language and protocol parity recorded in the [feature matrix](feature-matrix.md). Add shared fixtures for streamed tool deltas, opaque provider state, interruption recovery and feature rejection. Full C++ catalog parsing and long-lived process and connection testing remain open.
- Verify browser execution and the actual installable package on each supported platform. Record upstream tests using the [credentialed probe](live-verification.md) and additional capability-specific checks.
- Measure process RSS, reconnect behavior, long-running memory use, and deployment-target performance under representative load. Extend the manifest scan to target binaries and the production environment before general availability.
- Obtain separate approval for public publishing and production rollout.
