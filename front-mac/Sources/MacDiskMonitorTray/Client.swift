import Foundation
import OSLog

enum ClientUpdate: Sendable {
    case connecting
    case connected(Snapshot)
    case disconnected(String)
}

actor SSEClient {
    private let backendURL: String
    private let logger: Logger
    private var task: Task<Void, Never>?

    init(backendURL: String) {
        self.backendURL = backendURL
        self.logger = Logger(subsystem: "com.maximofn.mac-disk-monitor", category: "client")
    }

    func start(onUpdate: @escaping @Sendable (ClientUpdate) async -> Void) {
        task?.cancel()
        let url = backendURL
        let log = logger
        task = Task.detached { [weak self] in
            await self?.runLoop(streamURL: Self.streamURL(from: url), logger: log, onUpdate: onUpdate)
        }
    }

    func stop() {
        task?.cancel()
        task = nil
    }

    private nonisolated func runLoop(
        streamURL: URL,
        logger: Logger,
        onUpdate: @escaping @Sendable (ClientUpdate) async -> Void
    ) async {
        var backoffSeconds: UInt64 = 1
        await onUpdate(.connecting)

        while !Task.isCancelled {
            do {
                try await openOnce(streamURL: streamURL, logger: logger, backoff: &backoffSeconds, onUpdate: onUpdate)
                await onUpdate(.disconnected("stream closed by server"))
            } catch is CancellationError {
                return
            } catch {
                logger.warning("SSE session ended: \(error.localizedDescription, privacy: .public)")
                await onUpdate(.disconnected(error.localizedDescription))
            }

            do {
                try await Task.sleep(nanoseconds: backoffSeconds * 1_000_000_000)
            } catch {
                return
            }
            backoffSeconds = min(backoffSeconds * 2, 5)
        }
    }

    /// One SSE connection lifetime. Resets backoff on the first successful event.
    private nonisolated func openOnce(
        streamURL: URL,
        logger: Logger,
        backoff: inout UInt64,
        onUpdate: @escaping @Sendable (ClientUpdate) async -> Void
    ) async throws {
        logger.info("connecting to \(streamURL.absoluteString, privacy: .public)")
        var req = URLRequest(url: streamURL)
        req.setValue("text/event-stream", forHTTPHeaderField: "Accept")
        req.timeoutInterval = 30

        let (bytes, response) = try await URLSession.shared.bytes(for: req)
        if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
            throw NSError(
                domain: "SSEClient",
                code: http.statusCode,
                userInfo: [NSLocalizedDescriptionKey: "HTTP \(http.statusCode)"]
            )
        }

        let decoder = JSONDecoder()
        // SSE frames events with one or more `data:` lines followed by a blank
        // line. Foundation's `bytes.lines` collapses consecutive `\n`s though,
        // so the blank flush trigger never arrives — decode after every `data:`
        // instead. mac-disk-monitord ships one Snapshot per event (axum's
        // Event::default().json_data) so the JSON is self-contained.
        var buffer = ""

        for try await line in bytes.lines {
            if Task.isCancelled { throw CancellationError() }
            if line.hasPrefix(":") { continue }
            if line.hasPrefix("data:") {
                var payload = line.dropFirst(5)
                if payload.first == " " { payload = payload.dropFirst() }
                if !buffer.isEmpty { buffer.append("\n") }
                buffer.append(String(payload))

                if let data = buffer.data(using: .utf8) {
                    do {
                        let snap = try decoder.decode(Snapshot.self, from: data)
                        backoff = 1
                        await onUpdate(.connected(snap))
                        buffer.removeAll(keepingCapacity: true)
                    } catch {
                        if buffer.count > 1_000_000 {
                            logger.warning("SSE buffer >1MB without decode; resetting")
                            buffer.removeAll(keepingCapacity: false)
                        }
                    }
                }
            }
        }
    }

    private static func streamURL(from base: String) -> URL {
        var trimmed = base
        while trimmed.hasSuffix("/") { trimmed.removeLast() }
        return URL(string: trimmed + "/v1/stream")!
    }

    static func snapshotURL(from base: String) -> URL {
        var trimmed = base
        while trimmed.hasSuffix("/") { trimmed.removeLast() }
        return URL(string: trimmed + "/v1/snapshot")!
    }

    static func rescanURL(from base: String, mountPoint: String) -> URL {
        var trimmed = base
        while trimmed.hasSuffix("/") { trimmed.removeLast() }
        let stripped = mountPoint.hasPrefix("/") ? String(mountPoint.dropFirst()) : mountPoint
        let encoded = stripped.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? stripped
        return URL(string: trimmed + "/v1/rescan/" + encoded)!
    }
}
