use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    pub timestamp: String,
    pub host: String,
    pub mounts: Vec<Mount>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Mount {
    /// Mount point as reported by the kernel (e.g. `/`, `/Volumes/External`).
    pub mount_point: String,
    /// Block device backing this mount (e.g. `/dev/disk3s1s1`).
    pub device: String,
    /// Filesystem type (`apfs`, `hfs`, `exfat`, ...).
    pub fs_type: String,
    pub usage: Usage,
    /// Top-N largest files on this filesystem. Empty until the first scan
    /// completes (scans run in the background to avoid blocking the daemon).
    /// Files we cannot stat (EACCES from TCC, ENOENT during the walk) are skipped.
    #[serde(default)]
    pub largest_files: Vec<FileEntry>,
    /// RFC3339 timestamp of when `largest_files` was last refreshed; `None`
    /// means no scan has completed yet.
    #[serde(default)]
    pub largest_files_scanned_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub total_bytes: u64,
}

impl Usage {
    pub fn used_percent(&self) -> f32 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.used_bytes as f32 / self.total_bytes as f32) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_roundtrips_through_json() {
        let snap = Snapshot {
            timestamp: "2026-05-10T11:00:00Z".to_string(),
            host: "macbook".into(),
            mounts: vec![Mount {
                mount_point: "/".into(),
                device: "/dev/disk3s1s1".into(),
                fs_type: "apfs".into(),
                usage: Usage {
                    used_bytes: 50 * 1024 * 1024 * 1024,
                    free_bytes: 450 * 1024 * 1024 * 1024,
                    total_bytes: 500 * 1024 * 1024 * 1024,
                },
                largest_files: vec![FileEntry {
                    path: "/Users/foo/Movies/big.mov".into(),
                    size_bytes: 1_073_741_824,
                }],
                largest_files_scanned_at: Some("2026-05-10T11:30:00Z".into()),
            }],
        };
        let json = serde_json::to_string(&snap).unwrap();
        let back: Snapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap, back);
    }

    #[test]
    fn old_snapshot_without_largest_files_still_decodes() {
        let json = r#"{
            "timestamp":"2026-05-10T11:00:00Z",
            "host":"x",
            "mounts":[{
                "mount_point":"/",
                "device":"/dev/disk3s1s1",
                "fs_type":"apfs",
                "usage":{"used_bytes":0,"free_bytes":0,"total_bytes":0}
            }]
        }"#;
        let snap: Snapshot = serde_json::from_str(json).unwrap();
        assert!(snap.mounts[0].largest_files.is_empty());
        assert!(snap.mounts[0].largest_files_scanned_at.is_none());
    }

    #[test]
    fn used_percent_handles_zero_total() {
        let u = Usage::default();
        assert_eq!(u.used_percent(), 0.0);
    }

    #[test]
    fn used_percent_computes_correctly() {
        let u = Usage {
            used_bytes: 25,
            free_bytes: 75,
            total_bytes: 100,
        };
        assert!((u.used_percent() - 25.0).abs() < f32::EPSILON);
    }
}
