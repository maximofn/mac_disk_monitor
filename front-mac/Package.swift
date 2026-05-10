// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "MacDiskMonitorTray",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "MacDiskMonitorTray",
            resources: [.process("Resources")]
        )
    ]
)
