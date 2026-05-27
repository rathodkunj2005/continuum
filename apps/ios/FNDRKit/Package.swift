// swift-tools-version:5.10
//
// FNDRKit — shared library for the FNDR iPhone and Apple Watch companion apps.
//
// Contains the cross-platform glue: HTTP client, TLS pinning, JSON DTOs,
// Keychain wrapper, pairing state machine, connection-status poller.
// Pure data + networking; no SwiftUI.
//
// Two test entry points:
//   * `swift run FNDRKitCheck`  — runs the bundled assertion-based tests
//                                  on macOS without needing Xcode/XCTest.
//   * The `Tests/FNDRKitTests/` XCTest cases are kept for Xcode-based
//     iOS Simulator runs (they require full Xcode).

import PackageDescription

let package = Package(
    name: "FNDRKit",
    platforms: [
        .iOS(.v17),
        .watchOS(.v10),
        .macOS(.v14),
    ],
    products: [
        .library(name: "FNDRKit", targets: ["FNDRKit"]),
        .executable(name: "FNDRKitCheck", targets: ["FNDRKitCheck"]),
    ],
    targets: [
        .target(
            name: "FNDRKit",
            path: "Sources/FNDRKit"
        ),
        .executableTarget(
            name: "FNDRKitCheck",
            dependencies: ["FNDRKit"],
            path: "Sources/FNDRKitCheck"
        ),
        .testTarget(
            name: "FNDRKitTests",
            dependencies: ["FNDRKit"],
            path: "Tests/FNDRKitTests"
        ),
    ]
)
