// swift-tools-version:6.0

import PackageDescription

// The public model catalog SDK: the developer interface for
// https://models.lingxifox.cn/models.json.
//
// Foundation-only by contract. It must not gain a dependency on LingXiAgent,
// LingXiCore, LingXiApplication, LingXiClient, LingXiProtocol or
// LingXiPluginSDK — a consumer who wants a model's context window should not
// have to install an agent runtime. The platform floor is set by what the code
// actually uses (URLSession, FileManager, actors), not by the host product.
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
        )
    ],
    targets: [
        .target(
            name: "LingXiModelSDK"
        ),
        .testTarget(
            name: "LingXiModelSDKTests",
            dependencies: ["LingXiModelSDK"]
        )
    ]
)
