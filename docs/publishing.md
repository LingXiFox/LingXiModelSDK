# Publishing & Release Checklist

All package registry publications require explicit approval and must pass `./scripts/test-all.sh` first.

| Component | Target Registry | Verification Command |
| --- | --- | --- |
| Swift SDK | Swift Package Manager (Git tag) | `swift test` |
| Rust (`lingxi-model-sdk`, `lingxi-core`) | crates.io | `cargo test` & `cargo package --allow-dirty` |
| TypeScript (`@lingxifox/model-sdk`) | npm | `node --test tests/*.test.ts` & `npm pack` |
| Python (`lingxi-model-sdk`) | PyPI | `PYTHONPATH=src python3 -m unittest discover -s tests -v` & `pip wheel .` |
| Go (`sdk/go`, `gateway`, `website`) | Go module proxy / Git tags | `go vet ./... && go test -race ./...` |
| Java (`cn.lingxifox:lingxi-model-sdk`) | Maven Central | `CatalogTestRunner` + `ProviderTestRunner` & `javac --release 22` |
| Kotlin (`cn.lingxifox:lingxi-model-sdk-kotlin`) | Maven Central | `kotlinc -include-runtime` + `TestRunner` |
| C# (`LingXiModelSDK`) | NuGet | `dotnet pack` + a consumer project referencing the package |
| C++20 (`lingxi_cpp_sdk`) | CMake package / source | `cmake --install` + `find_package(lingxi_cpp_sdk)` consumer |
| Dart (`lingxi_model_sdk`) | pub.dev | `dart analyze` + a `path` dependency consumer |

Use `.github/workflows/release-artifacts.yml` (`workflow_dispatch`) to stage the [alpha candidate bundle](release-2.0.0-alpha.1.md) prior to any release.

## Locally verified artifacts

Each artifact below was produced from this working tree, installed into an empty project, and exercised through its public entry points. A build that merely compiles is not listed. Nothing was uploaded to a public registry.

| Artifact | Command | Consumer check |
| --- | --- | --- |
| npm tarball `lingxifox-model-sdk-2.0.0-alpha.1.tgz` | `npm pack` in `sdk/typescript` | `npm install` in an empty project, then import all four export subpaths and call `decodeCatalog`, `NativeClient`, `translateRequest` and `ProviderRegistry.fromJSON` |
| Python wheel `lingxi_model_sdk-2.0.0a1-py3-none-any.whl` | `pip3 wheel . --no-deps` in `sdk/python` | `pip install` into a fresh `venv`, then decode the catalog fixture, translate a request and normalize a stream frame through the bundled ctypes bridge with `LINGXI_CORE_LIB` pointing at the shared library |
| NuGet package `LingXiModelSDK.2.0.0-alpha.1.nupkg` | `dotnet pack -c Release` | empty console project with a `PackageReference`, then `LingXiModelCatalog.Decode`, `CoreBridge.Available` and `TranslateRequest` |
| JAR `lingxi-model-sdk.jar` | `javac --release 22` + `jar cf` | separate `Consumer.java` compiled against the JAR, then catalog decode and `RustCoreBridge.translateRequest` |
| Kotlin JAR `lingxi-kotlin.jar` | `kotlinc -jvm-target 22 -include-runtime` | separate `Consumer.kt` compiled against the JAR, then catalog decode and `KotlinBridge.translateRequest` |
| CMake package `lingxi_cpp_sdk` | `cmake --install --prefix <dir>` | empty project using `find_package(lingxi_cpp_sdk REQUIRED)` and `target_link_libraries(... lingxi::lingxi_cpp_catalog)`, then `ModelCatalog::decode` and provider ordering |
| Dart package `lingxi_model_sdk` | `dart pub get` in `sdk/dart` | empty project with a `path` dependency, then `LingXiModelCatalog.decode` and `CoreBridge.shared().translateRequest` |
| Rust crates `lingxi-core` and `lingxi-model-sdk` | `cargo package --allow-dirty` | both `.crate` archives are produced from the committed lockfile contents |

The C++ SDK is header-only; it does not embed the Rust core. Consumers link `liblingxi_core` themselves, exactly as the verified consumer project does.
