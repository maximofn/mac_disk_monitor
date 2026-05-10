import Foundation

// Mirror of crates/mac-disk-monitor-core/src/model.rs. The Rust types are the
// canonical schema (API path /v1/...). If a field is added there, replicate it
// here verbatim or the JSON decode will silently drop data.

struct Snapshot: Codable, Equatable, Sendable {
    let timestamp: String
    let host: String
    let mounts: [Mount]
}

struct Mount: Codable, Equatable, Sendable {
    let mountPoint: String
    let device: String
    let fsType: String
    let usage: Usage
    let largestFiles: [FileEntry]
    let largestFilesScannedAt: String?

    enum CodingKeys: String, CodingKey {
        case mountPoint = "mount_point"
        case device
        case fsType = "fs_type"
        case usage
        case largestFiles = "largest_files"
        case largestFilesScannedAt = "largest_files_scanned_at"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        mountPoint = try c.decode(String.self, forKey: .mountPoint)
        device = try c.decode(String.self, forKey: .device)
        fsType = try c.decode(String.self, forKey: .fsType)
        usage = try c.decode(Usage.self, forKey: .usage)
        largestFiles = (try? c.decode([FileEntry].self, forKey: .largestFiles)) ?? []
        largestFilesScannedAt = try? c.decode(String.self, forKey: .largestFilesScannedAt)
    }
}

struct FileEntry: Codable, Equatable, Sendable {
    let path: String
    let sizeBytes: UInt64

    enum CodingKeys: String, CodingKey {
        case path
        case sizeBytes = "size_bytes"
    }
}

struct Usage: Codable, Equatable, Sendable {
    let usedBytes: UInt64
    let freeBytes: UInt64
    let totalBytes: UInt64

    enum CodingKeys: String, CodingKey {
        case usedBytes = "used_bytes"
        case freeBytes = "free_bytes"
        case totalBytes = "total_bytes"
    }

    var usedPercent: Float {
        guard totalBytes > 0 else { return 0 }
        return Float(usedBytes) / Float(totalBytes) * 100.0
    }
}
