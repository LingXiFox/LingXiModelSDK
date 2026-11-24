# LingXi Optional HTTP Gateway (`gateway/`)

Optional self-hosted Go 1.23+ HTTP/SSE server wrapping the Unified LMP & Provider Registry layers.

```sh
cd gateway
go vet ./...
go test -race -v ./...
CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o lingxi-gateway .
./lingxi-gateway -listen 127.0.0.1:18090 -registry ../registry/registry.json
```

See [specs/gateway-v1.md](../specs/gateway-v1.md) for security defaults and endpoints.
