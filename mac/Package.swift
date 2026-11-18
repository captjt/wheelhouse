// swift-tools-version:6.2
// Wheelhouse.app: the native Mac app for the hub (`wh serve`). `./build.sh`
// builds it into Wheelhouse.app; `just app` builds and installs it.
import PackageDescription

let package = Package(
    name: "Wheelhouse",
    platforms: [.macOS(.v14)],
    dependencies: [
        // Markdown as the web app renders it: CommonMark and GitHub's tables,
        // task lists, strikethrough and autolinks (cmark-gfm).
        .package(url: "https://github.com/swiftlang/swift-markdown.git", .upToNextMinor(from: "0.7.3")),
    ],
    targets: [
        .executableTarget(
            name: "Wheelhouse",
            dependencies: [.product(name: "Markdown", package: "swift-markdown")],
            path: "Sources/Wheelhouse",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .testTarget(
            name: "WheelhouseTests",
            dependencies: ["Wheelhouse"],
            path: "Tests/WheelhouseTests",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
