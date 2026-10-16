// swift-tools-version:5.9
// The Ally menu bar app. It only draws what `ally internal-watch` prints, so all
// polling and every decision to notify stays in the Rust program.

import PackageDescription

let package = Package(
    name: "AllyMenuBar",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(name: "AllyMenuBar", path: "Sources")
    ]
)
