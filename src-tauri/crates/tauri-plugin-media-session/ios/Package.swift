// swift-tools-version:5.5

import PackageDescription

let package = Package(
    name: "tauri-plugin-media-session",
    platforms: [
        .iOS(.v13),
    ],
    products: [
        .library(
            name: "tauri-plugin-media-session",
            type: .static,
            targets: ["tauri-plugin-media-session"]
        ),
    ],
    dependencies: [
        .package(name: "Tauri", path: "../.tauri/tauri-api"),
    ],
    targets: [
        .target(
            name: "tauri-plugin-media-session",
            dependencies: [
                .byName(name: "Tauri"),
            ],
            path: "Sources"
        ),
    ]
)
