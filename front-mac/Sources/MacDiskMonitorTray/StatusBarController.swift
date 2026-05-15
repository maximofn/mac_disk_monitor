import AppKit
import Foundation
import OSLog

private let repoURL = URL(string: "https://github.com/maximofn/mac_disk_monitor")!
private let coffeeURL = URL(string: "https://www.buymeacoffee.com/maximofn")!

enum TrayState: Sendable {
    case connecting
    case connected(Snapshot)
    case disconnected(String)
}

@MainActor
final class StatusBarController: NSObject {
    private let statusItem: NSStatusItem
    private let renderer: IconRenderer
    private let backendURL: String
    private let logger = Logger(subsystem: "com.maximofn.mac-disk-monitor", category: "tray")
    private var state: TrayState = .connecting
    private var lastAppearance: IconAppearance = .dark
    private var lastRenderedKey: String = ""
    private let compactModeDefaultsKey = "MacDiskMonitorTray.compactMode"
    private var compactMode: Bool

    init(renderer: IconRenderer, backendURL: String) {
        self.renderer = renderer
        self.backendURL = backendURL
        self.statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        self.compactMode = UserDefaults.standard.bool(forKey: compactModeDefaultsKey)
        super.init()
        if let button = statusItem.button {
            button.imagePosition = .imageLeft
            button.toolTip = "Mac Disk Monitor — connecting to \(backendURL)"
        }
        // System-wide light/dark toggle. Don't KVO `effectiveAppearance` on the
        // button — AppKit re-evaluates that during repaints and any reaction
        // there feeds back into refreshIcon → set image → repaint → KVO loop.
        DistributedNotificationCenter.default.addObserver(
            self,
            selector: #selector(appearanceChanged),
            name: Notification.Name("AppleInterfaceThemeChangedNotification"),
            object: nil
        )
        lastAppearance = currentAppearance
        applyState(.connecting)
    }

    deinit {
        DistributedNotificationCenter.default.removeObserver(self)
    }

    @objc private func appearanceChanged() {
        Task { @MainActor in
            self.lastAppearance = self.currentAppearance
            self.lastRenderedKey = ""
            self.refreshIcon()
        }
    }

    func applyState(_ new: TrayState) {
        state = new
        refreshIcon()
        refreshMenu()
        refreshTooltip()
    }

    private var currentAppearance: IconAppearance {
        let appearance = statusItem.button?.effectiveAppearance ?? NSApp.effectiveAppearance
        let match = appearance.bestMatch(from: [.darkAqua, .vibrantDark, .aqua, .vibrantLight])
        switch match {
        case .darkAqua, .vibrantDark: return .dark
        default: return .light
        }
    }

    private func refreshIcon() {
        let (mounts, connected): ([Mount], Bool) = {
            switch state {
            case .connected(let snap): return (snap.mounts, true)
            default: return ([], false)
            }
        }()
        // Dedupe identical renders — at 1 Hz most ticks have identical visible state.
        let key = renderKey(mounts: mounts, connected: connected, appearance: lastAppearance)
        if key == lastRenderedKey { return }
        lastRenderedKey = key
        if let img = renderer.renderImage(mounts: mounts, connected: connected, appearance: lastAppearance, compact: compactMode) {
            statusItem.button?.image = img
        }
    }

    private func renderKey(mounts: [Mount], connected: Bool, appearance: IconAppearance) -> String {
        var parts: [String] = ["\(connected)", "\(appearance)", "compact=\(compactMode)"]
        for m in mounts {
            let pct = Int(m.usage.usedPercent.rounded())
            parts.append("\(m.mountPoint):\(pct)")
        }
        return parts.joined(separator: "|")
    }

    private func refreshTooltip() {
        guard let button = statusItem.button else { return }
        switch state {
        case .connecting:
            button.toolTip = "Mac Disk Monitor — connecting to \(backendURL)"
        case .connected(let snap):
            let header = "\(snap.mounts.count) mount(s) on \(snap.host)"
            let body = snap.mounts.map { m in
                let used = formatBytes(m.usage.usedBytes)
                let total = formatBytes(m.usage.totalBytes)
                return "\(m.mountPoint) — \(used) / \(total) (\(Int(m.usage.usedPercent.rounded()))%)"
            }.joined(separator: "\n")
            button.toolTip = "\(header)\n\(body)"
        case .disconnected(let err):
            button.toolTip = "Backend offline: \(err)"
        }
    }

    private func refreshMenu() {
        let menu = NSMenu()
        menu.autoenablesItems = false

        switch state {
        case .connecting:
            menu.addItem(disabledItem("Connecting to \(backendURL)…"))
            menu.addItem(.separator())
        case .disconnected(let err):
            menu.addItem(disabledItem("Backend offline: \(err)"))
            menu.addItem(disabledItem("Backend: \(backendURL)"))
            menu.addItem(.separator())
        case .connected(let snap):
            for mount in snap.mounts {
                let title = String(
                    format: "%@ — %d%% (%@/%@)",
                    mount.mountPoint as NSString,
                    Int(mount.usage.usedPercent.rounded()),
                    formatBytes(mount.usage.usedBytes) as NSString,
                    formatBytes(mount.usage.totalBytes) as NSString
                )
                let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
                item.submenu = mountSubmenu(for: mount)
                menu.addItem(item)
            }
            menu.addItem(.separator())
            menu.addItem(disabledItem("Backend: \(backendURL)"))
            menu.addItem(disabledItem("Updated: \(shortTime(snap.timestamp))"))
            menu.addItem(.separator())
        }

        let toggleTitle = compactMode ? "Cambiar a extendido" : "Cambiar a compacto"
        let toggle = NSMenuItem(title: toggleTitle, action: #selector(toggleCompactMode), keyEquivalent: "")
        toggle.target = self
        menu.addItem(toggle)
        menu.addItem(.separator())

        let repo = NSMenuItem(title: "Repository", action: #selector(openRepo), keyEquivalent: "")
        repo.target = self
        menu.addItem(repo)
        let coffee = NSMenuItem(title: "Buy me a coffee", action: #selector(openCoffee), keyEquivalent: "")
        coffee.target = self
        menu.addItem(coffee)
        menu.addItem(.separator())
        let quit = NSMenuItem(title: "Quit", action: #selector(quit), keyEquivalent: "q")
        quit.target = self
        menu.addItem(quit)

        statusItem.menu = menu
    }

    private func mountSubmenu(for mount: Mount) -> NSMenu {
        let m = NSMenu()
        m.autoenablesItems = false
        m.addItem(disabledItem("Device: \(mount.device)"))
        m.addItem(disabledItem("Filesystem: \(mount.fsType)"))
        m.addItem(disabledItem("Used: \(formatBytes(mount.usage.usedBytes))"))
        m.addItem(disabledItem("Free: \(formatBytes(mount.usage.freeBytes))"))
        m.addItem(disabledItem(
            "Total: \(formatBytes(mount.usage.totalBytes)) (\(Int(mount.usage.usedPercent.rounded()))% used)"
        ))

        m.addItem(.separator())
        if mount.largestFiles.isEmpty {
            if let scanned = mount.largestFilesScannedAt {
                m.addItem(disabledItem("Largest files: none reported (scanned \(shortTime(scanned)))"))
            } else {
                m.addItem(disabledItem("Largest files: scanner disabled — pass --largest-top-n on the daemon"))
            }
        } else {
            m.addItem(disabledItem("Largest files (\(mount.largestFiles.count))"))
            for f in mount.largestFiles {
                let line = "  \(formatBytes(f.sizeBytes))  \(f.path)"
                m.addItem(disabledItem(line))
            }
            if let scanned = mount.largestFilesScannedAt {
                m.addItem(disabledItem("(scanned \(shortTime(scanned)))"))
            }
        }

        m.addItem(.separator())
        let rescan = NSMenuItem(
            title: "Rescan largest files",
            action: #selector(rescanMount(_:)),
            keyEquivalent: ""
        )
        rescan.target = self
        rescan.representedObject = mount.mountPoint
        m.addItem(rescan)

        return m
    }

    @objc private func openRepo() { NSWorkspace.shared.open(repoURL) }
    @objc private func openCoffee() { NSWorkspace.shared.open(coffeeURL) }
    @objc private func quit() { NSApp.terminate(nil) }

    @objc private func toggleCompactMode() {
        compactMode.toggle()
        UserDefaults.standard.set(compactMode, forKey: compactModeDefaultsKey)
        lastRenderedKey = ""
        refreshIcon()
        refreshMenu()
    }

    @objc private func rescanMount(_ sender: NSMenuItem) {
        guard let mountPoint = sender.representedObject as? String else { return }
        let url = SSEClient.rescanURL(from: backendURL, mountPoint: mountPoint)
        let log = logger
        Task.detached {
            var req = URLRequest(url: url)
            req.httpMethod = "POST"
            req.timeoutInterval = 5
            do {
                let (_, response) = try await URLSession.shared.data(for: req)
                if let http = response as? HTTPURLResponse {
                    log.info("rescan \(mountPoint, privacy: .public) → HTTP \(http.statusCode, privacy: .public)")
                }
            } catch {
                log.warning("rescan \(mountPoint, privacy: .public) failed: \(error.localizedDescription, privacy: .public)")
            }
        }
    }
}

// MARK: - Helpers

private func disabledItem(_ title: String) -> NSMenuItem {
    let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
    item.isEnabled = false
    return item
}

private func formatBytes(_ bytes: UInt64) -> String {
    let tib: Double = 1024 * 1024 * 1024 * 1024
    let gib: Double = 1024 * 1024 * 1024
    let mib: Double = 1024 * 1024
    let kib: Double = 1024
    let b = Double(bytes)
    if b >= tib { return String(format: "%.2f TiB", b / tib) }
    if b >= gib { return String(format: "%.2f GiB", b / gib) }
    if b >= mib { return String(format: "%.0f MiB", b / mib) }
    if b >= kib { return String(format: "%.0f KiB", b / kib) }
    return "\(bytes) B"
}

/// "2026-05-06T10:11:12.345Z" → "10:11:12".
private func shortTime(_ rfc3339: String) -> String {
    guard let tIdx = rfc3339.firstIndex(of: "T") else { return rfc3339 }
    let after = rfc3339[rfc3339.index(after: tIdx)...]
    if let dot = after.firstIndex(of: ".") {
        return String(after[..<dot])
    }
    if let plus = after.firstIndex(where: { $0 == "+" || $0 == "Z" || $0 == "-" }) {
        return String(after[..<plus])
    }
    return String(after)
}
