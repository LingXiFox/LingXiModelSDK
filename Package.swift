// swift-tools-version:6.0

import PackageDescription

// The public model catalog SDK: the developer interface for
// https://models.lingxifox.cn/models.json.
//
// Foundation-only by contract. Catalog (`LingXiModelSDK`) remains completely
// independent from Registry, Native, and Unified provider targets.
let package = Package(
    name: "LingXiModelSDK",
    platforms: [
        .macOS(.v13),
        .iOS(.v16),
        .tvOS(.v16),
        .watchOS(.v9),
    ],
    products: [
        .library(
            name: "LingXiModelSDK",
            targets: ["LingXiModelSDK"]
        ),
        .library(
            name: "LingXiProviderRegistry",
            targets: ["LingXiProviderRegistry"]
        ),
        .library(
            name: "LingXiNativeProvider",
            targets: ["LingXiNativeProvider"]
        ),
        .library(
            name: "LingXiUnifiedProvider",
            targets: ["LingXiUnifiedProvider"]
        ),
    ],
    targets: [
        .target(
            name: "LingXiModelSDK"
        ),
        .target(
            name: "LingXiProviderRegistry"
        ),
        .target(
            name: "LingXiNativeProvider",
            dependencies: ["LingXiProviderRegistry"]
        ),
        .target(
            name: "LingXiUnifiedProvider",
            dependencies: ["LingXiProviderRegistry", "LingXiNativeProvider"]
        ),
        .testTarget(
            name: "LingXiModelSDKTests",
            dependencies: ["LingXiModelSDK"]
        ),
        .testTarget(
            name: "LingXiProviderTests",
            dependencies: ["LingXiProviderRegistry", "LingXiNativeProvider", "LingXiUnifiedProvider"]
        ),
    ]
)
