import Foundation

struct Config: Sendable {
    var backendURL: String
    var iconHeight: CGFloat
    var logLevel: String
    var dumpIcon: String?

    static let `default` = Config(
        backendURL: ProcessInfo.processInfo.environment["MAC_DISK_MONITOR_TRAY_URL"]
            ?? "http://127.0.0.1:9136",
        iconHeight: 22,
        logLevel: "info",
        dumpIcon: nil
    )

    static func parse(_ args: [String]) -> Config {
        var cfg = Config.default
        var i = 1
        while i < args.count {
            let arg = args[i]
            switch arg {
            case "--backend-url":
                i += 1
                if i < args.count { cfg.backendURL = args[i] }
            case "--icon-height":
                i += 1
                if i < args.count, let v = Double(args[i]) { cfg.iconHeight = CGFloat(v) }
            case "--log-level":
                i += 1
                if i < args.count { cfg.logLevel = args[i] }
            case "--dump-icon":
                i += 1
                if i < args.count { cfg.dumpIcon = args[i] }
            case "-h", "--help":
                Self.printUsage()
                exit(0)
            case "--version":
                print("mac-disk-monitor-tray 0.1.0")
                exit(0)
            default:
                FileHandle.standardError.write(Data("unknown argument: \(arg)\n".utf8))
                Self.printUsage()
                exit(2)
            }
            i += 1
        }
        return cfg
    }

    static func printUsage() {
        let usage = """
        mac-disk-monitor-tray — macOS menu bar frontend for mac-disk-monitord

        USAGE:
            mac-disk-monitor-tray [OPTIONS]

        OPTIONS:
            --backend-url <URL>     Base URL of mac-disk-monitord (default: http://127.0.0.1:9136,
                                    env: MAC_DISK_MONITOR_TRAY_URL)
            --icon-height <PT>      Status bar icon height in points (default: 22)
            --log-level <LEVEL>     trace|debug|info|warn|error (default: info)
            --dump-icon <PATH>      Render one snapshot to a PNG and exit
            --version               Print version and exit
            -h, --help              Print this help

        """
        print(usage)
    }
}
