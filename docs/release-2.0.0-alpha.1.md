# LingXiModelSDK 2.0.0-alpha.1 Candidate

The candidate covers the SDK, provider registry, optional Gateway, and public catalog site. The [feature matrix](feature-matrix.md) records supported paths and gaps. Provider verification remains at 9 mock-tested, 9 declared compatible, and 309 discovered. No provider has credentialed verification evidence.

## Stage and validate

Run the [release artifact workflow](../.github/workflows/release-artifacts.yml) for the candidate commit. It builds Linux amd64 and arm64 website and Gateway binaries, the Linux x86_64 Rust shared library and C ABI header, and the npm tarball. The bundle includes the provider registry, license, release notes, `COMMIT.txt`, and `SHA256SUMS.txt`.

Download the workflow artifact, compare `COMMIT.txt` with the approved candidate SHA, and verify the files with `sha256sum -c SHA256SUMS.txt`. Use the binary matching the target host architecture and stage it with a separate service port and data directory.

## Website rollout and rollback

Follow [the deployment runbook](../website/deploy/DEPLOYMENT.md) with separate production approval. Validate the staged binary against a copy of the last-known-good catalog before switching traffic. Verify conditional reads, filing icon, and the SDK, protocol, provider and Gateway pages. Preserve the previous route configuration and binary until the new service has passed production acceptance.

For rollback, restore the saved Caddy site configuration and hot-reload Caddy. If the rollout replaced an existing Go binary, restore the previous binary via a same-filesystem rename, restart its service, and repeat the public catalog checks.
