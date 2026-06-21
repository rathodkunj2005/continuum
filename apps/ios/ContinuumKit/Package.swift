// swift-tools-version:5.10
//
// ContinuumKit — shared library for the Continuum iPhone and Apple Watch companion apps.
//
// Contains the cross-platform glue: HTTP client, TLS pinning, JSON DTOs,
// Keychain wrapper, pairing state machine, connection-status poller.
// Pure data + networking; no SwiftUI.
//
// Two test entry points:
//   * `swift run ContinuumKitCheck`  — runs the bundled assertion-based tests
//                                  on macOS without needing Xcode/XCTest.
//   * The `Tests/ContinuumKitTests/` XCTest cases are kept for Xcode-based
//     iOS Simulator runs (they require full Xcode).

import PackageDescription

let package = Package(
    name: "ContinuumKit",
    platforms: [
        .iOS(.v17),
        .watchOS(.v10),
        .macOS(.v14),
    ],
    products: [
        .library(name: "ContinuumKit", targets: ["ContinuumKit"]),
        .executable(name: "ContinuumKitCheck", targets: ["ContinuumKitCheck"]),
    ],
    targets: [
        .target(
            name: "ContinuumKit",
            path: "Sources/ContinuumKit"
        ),
        .executableTarget(
            name: "ContinuumKitCheck",
            dependencies: ["ContinuumKit"],
            path: "Sources/ContinuumKitCheck"
        ),
        .testTarget(
            name: "ContinuumKitTests",
            dependencies: ["ContinuumKit"],
            path: "Tests/ContinuumKitTests"
        ),
    ]
)
