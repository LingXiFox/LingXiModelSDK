# LingXiModelSDK · Java Catalog SDK

Zero-dependency Java 17+ client for the LingXi public model catalog (`https://models.lingxifox.cn/models.json`).

```sh
cd sdk/java
mkdir -p build/classes
javac -d build/classes src/main/java/cn/lingxifox/modelsdk/*.java src/test/java/cn/lingxifox/modelsdk/*.java
java -cp build/classes cn.lingxifox.modelsdk.CatalogTestRunner
```
