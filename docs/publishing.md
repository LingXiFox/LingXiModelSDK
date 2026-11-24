# Publishing & Release Checklist

All package registry publications require explicit approval and must pass `./scripts/test-all.sh` first.

| Component | Target Registry | Verification Command |
| --- | --- | --- |
| Swift SDK | Swift Package Manager (Git tag) | `swift test` |
| Rust (`lingxi-model-sdk`, `lingxi-core`) | crates.io | `cargo test` & `cargo publish --dry-run` |
| TypeScript (`@lingxifox/model-sdk`) | npm | `node --test tests/*.test.ts` & `npm publish --dry-run` |
| Python (`lingxi-model-sdk`) | PyPI | `PYTHONPATH=src python3 -m unittest discover -s tests -v` |
| Go (`sdk/go`, `gateway`, `website`) | Go module proxy / Git tags | `go vet ./... && go test -race ./...` |
| Java (`cn.lingxifox:lingxi-model-sdk`) | Maven Central | `CatalogTestRunner` & `mvn verify` |

Use `.github/workflows/release-artifacts.yml` (`workflow_dispatch`) to stage the [alpha candidate bundle](release-2.0.0-alpha.1.md) prior to any release.
