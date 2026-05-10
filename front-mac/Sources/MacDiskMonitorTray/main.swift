import AppKit
import Foundation

let config = Config.parse(CommandLine.arguments)

if let dumpPath = config.dumpIcon {
    // One-shot mode: fetch a single snapshot via /v1/snapshot, render to PNG,
    // exit. Synchronous Data(contentsOf:) on purpose — pinning Tasks to
    // MainActor while blocking the main thread on a semaphore deadlocks.
    let url = SSEClient.snapshotURL(from: config.backendURL)
    let renderer = IconRenderer(height: config.iconHeight)
    do {
        let data = try Data(contentsOf: url)
        let snap = try JSONDecoder().decode(Snapshot.self, from: data)
        try renderer.renderPNG(mounts: snap.mounts, connected: true, to: dumpPath)
        print("wrote \(dumpPath)")
        exit(0)
    } catch {
        FileHandle.standardError.write(
            Data("backend unreachable (\(error.localizedDescription)) — dumping disconnected icon\n".utf8)
        )
        do {
            try renderer.renderPNG(mounts: [], connected: false, to: dumpPath)
            print("wrote \(dumpPath) (disconnected)")
            exit(0)
        } catch {
            FileHandle.standardError.write(Data("error: \(error.localizedDescription)\n".utf8))
            exit(1)
        }
    }
}

let app = NSApplication.shared
let delegate = AppDelegate(config: config)
app.delegate = delegate
app.setActivationPolicy(.accessory)
app.run()
