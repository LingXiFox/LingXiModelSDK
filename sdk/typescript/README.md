# LingXiModelSDK · TypeScript / JavaScript Catalog SDK

Zero-dependency TypeScript/JavaScript client for the LingXi public model catalog (`https://models.lingxifox.cn/models.json`). The built ESM package runs on Node.js 20+. Catalog consumers can use a browser bundler. AWS SigV4 uses Node crypto. Run the build and tests with Node.js 24.

```sh
cd sdk/typescript
npm test
npm run build
```

The package root exports Catalog. Use `@lingxifox/model-sdk/registry`, `/native` and `/unified` through their package subpaths. `npm pack` builds a JavaScript tarball and includes the TypeScript source for type resolution.
