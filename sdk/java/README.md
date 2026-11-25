# LingXiModelSDK · Java SDK

Zero-dependency Java client for the LingXi model catalog, provider registry, native
providers and the unified LMP layer. Protocol knowledge lives in the Rust core;
`RustCoreBridge` reaches it through the C ABI with the Foreign Function & Memory API,
so the toolchain must be JDK 22+ (`java.lang.foreign` became final there).

Build the shared library first, then point the SDK at it:

```sh
cargo build --manifest-path ../../core/Cargo.toml
cd sdk/java
mkdir -p build/classes
javac --release 22 -d build/classes src/main/java/cn/lingxifox/modelsdk/*.java \
  src/test/java/cn/lingxifox/modelsdk/*.java
LINGXI_CORE_LIB=$PWD/../../core/target/debug/liblingxi_core.dylib \
  java --enable-native-access=ALL-UNNAMED -cp build/classes cn.lingxifox.modelsdk.CatalogTestRunner
LINGXI_CORE_LIB=$PWD/../../core/target/debug/liblingxi_core.dylib \
  java --enable-native-access=ALL-UNNAMED -cp build/classes cn.lingxifox.modelsdk.ProviderTestRunner
```

On Linux the shared library is `liblingxi_core.so` and the flag is not needed for
`cargo build`, only the same `LINGXI_CORE_LIB` override. The bridge also accepts
`-Dlingxi.core.lib=<path>` and falls back to a `core/target/{release,debug}` directory
discovered by walking up from the working directory.

The catalog classes alone (`LingXiModelCatalog`, `CatalogLoader`, `CatalogDecoder`) do
not touch the bridge and compile on older JDKs, but the published classes are built
together, so the artifact targets 22+.
