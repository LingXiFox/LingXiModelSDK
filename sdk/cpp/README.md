# LingXiModelSDK for C++20

Header-only C++20 binding for the Model Catalog, Provider Registry, Native Provider and Unified Provider. Wire protocol logic lives in the Rust core (`core/`); this package supplies a small JSON value type, the catalog parser and loader, and thin facades over the C ABI.

## Requirements

- C++20 compiler and CMake 3.20+
- The Rust core shared library (`liblingxi_core.dylib`, `liblingxi_core.so` or `lingxi_core.dll`) built from `core/` (`cargo build --release --manifest-path core/Cargo.toml`)

The catalog parser works without the core. The registry, native and unified modules need the shared library at run time.

## Building and installing

```sh
cmake -S sdk/cpp -B build/cpp -DCMAKE_BUILD_TYPE=Release
cmake --build build/cpp
ctest --test-dir build/cpp --output-on-failure
cmake --install build/cpp --prefix /path/to/prefix
```

A consumer project then uses:

```cmake
find_package(lingxi_cpp_sdk REQUIRED)
target_link_libraries(app PRIVATE lingxi::lingxi_cpp_catalog)
```

## Modules

| Header | Purpose |
| --- | --- |
| `lingxi/json.hpp` | Order-preserving JSON value with parse and dump |
| `lingxi/catalog.hpp` | Catalog decoding, ordering, search and filters |
| `lingxi/catalog_loader.hpp` | Offline cache with ETag revalidation and stale fallback |
| `lingxi/core.hpp` | C ABI entry points with typed `CoreError` |
| `lingxi/registry.hpp` | Registry endpoint resolution |
| `lingxi/native.hpp` | Prepared native wire targets (URL, headers, body, framing) |
| `lingxi/http.hpp` | Transport-injected native client and SSE/NDJSON frame splitting |
| `lingxi/unified.hpp` | LMP translation, preparation and normalization |
| `lingxi/stream.hpp` | Incremental stream sessions driven by the Rust core |
| `lingxi/normalizer.hpp` | Frame-at-a-time LMP normalization for host-owned transports |

`lingxi/unified.hpp` never fabricates a response: unsupported mappings surface as `CoreError` with the `unsupported_feature` code, matching the other SDKs.

## Catalog example

```cpp
#include <lingxi/catalog.hpp>

auto catalog = lingxi::catalog::ModelCatalog::decode(document);
auto gpt = catalog.model("prov10", "gpt-5");
auto ranked = catalog.sorted_providers({"prov10"});
```

## Streaming example

```cpp
#include <lingxi/stream.hpp>

lingxi::json::Value request = lingxi::json::Value::parse(R"({})");
auto session = lingxi::StreamSession::open(request);
while (auto frame = session.next(30'000)) {
  // frame is a provider frame (native) or an LMP event (unified).
}
```

`next()` blocks for up to the given timeout; `cancel()` is safe from another thread and releases the upstream connection.

## Tests

`sdk/cpp/tests/test_cpp_sdk.cpp` replays the shared `conformance/` fixtures against a loopback HTTP server and covers catalog decoding, registry resolution, all nine protocols through native and unified paths, embeddings, reranking, incremental streaming and cancellation.