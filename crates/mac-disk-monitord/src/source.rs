use std::collections::HashSet;

use anyhow::Result;
use mac_disk_monitor_core::{Mount, Usage};
use sysinfo::Disks;

pub trait DiskSource: Send + Sync {
    fn sample(&self) -> Result<Vec<Mount>>;
}

/// Real macOS disk source: enumerates mounted volumes via `sysinfo::Disks`,
/// which under the hood calls `getfsstat(2)` on Darwin. Filters out the
/// duplicate "system slice" volumes APFS exposes (`/System/Volumes/Preboot`,
/// `/System/Volumes/VM`, …) so the user only sees mounts whose space is
/// actually reportable as a unit.
pub struct SysinfoSource;

impl SysinfoSource {
    pub fn new() -> Self {
        Self
    }
}

impl DiskSource for SysinfoSource {
    fn sample(&self) -> Result<Vec<Mount>> {
        // `new_with_refreshed_list` re-reads the kernel mount table every call;
        // an external drive plug/unplug between samples therefore shows up
        // immediately without any extra plumbing.
        let disks = Disks::new_with_refreshed_list();

        let mut out: Vec<Mount> = Vec::new();
        let mut seen_devices: HashSet<String> = HashSet::new();

        for disk in disks.iter() {
            let mount_point = disk.mount_point().to_string_lossy().to_string();
            let device = disk.name().to_string_lossy().to_string();
            let fs_type = disk.file_system().to_string_lossy().to_string();
            let total = disk.total_space();
            let available = disk.available_space();

            if !is_real_filesystem(&mount_point, &fs_type, total) {
                continue;
            }

            // APFS containers expose multiple "volumes" backed by the same
            // physical device. After /, drop any subsequent mount whose device
            // name we've already accounted for — root usage is the canonical
            // figure for the container.
            if !seen_devices.insert(device.clone()) {
                continue;
            }

            let used = total.saturating_sub(available);
            out.push(Mount {
                mount_point,
                device,
                fs_type,
                usage: Usage {
                    used_bytes: used,
                    free_bytes: available,
                    total_bytes: total,
                },
                largest_files: Vec::new(),
                largest_files_scanned_at: None,
            });
        }

        // Stable ordering: root first, then alphabetical. Keeps the menu bar
        // donut layout deterministic frame-to-frame so it doesn't jitter.
        out.sort_by(|a, b| match (a.mount_point.as_str(), b.mount_point.as_str()) {
            ("/", _) => std::cmp::Ordering::Less,
            (_, "/") => std::cmp::Ordering::Greater,
            (x, y) => x.cmp(y),
        });
        Ok(out)
    }
}

const PSEUDO_FS: &[&str] = &[
    "devfs",
    "autofs",
    "ctl",
    "fdesc",
    "tmpfs",
    "msdos", // EFI System Partition; small and bootloader-managed.
];

/// Mount points that APFS / macOS expose but which share their backing storage
/// with `/` and are managed by the OS. Showing them just produces duplicate
/// donuts at the same percentage as root.
const HIDDEN_MOUNT_PREFIXES: &[&str] = &[
    "/System/Volumes/Preboot",
    "/System/Volumes/VM",
    "/System/Volumes/Update",
    "/System/Volumes/iSCPreboot",
    "/System/Volumes/xarts",
    "/System/Volumes/Hardware",
    "/System/Volumes/Recovery",
    "/private/var/vm",
];

fn is_real_filesystem(mount_point: &str, fs_type: &str, total_bytes: u64) -> bool {
    if total_bytes == 0 {
        return false;
    }
    if PSEUDO_FS.iter().any(|f| fs_type.eq_ignore_ascii_case(f)) {
        return false;
    }
    if HIDDEN_MOUNT_PREFIXES
        .iter()
        .any(|p| mount_point == *p || mount_point.starts_with(&format!("{p}/")))
    {
        return false;
    }
    true
}

#[cfg(test)]
pub struct MockSource {
    mounts: Vec<Mount>,
}

#[cfg(test)]
impl MockSource {
    pub fn new(mounts: Vec<Mount>) -> Self {
        Self { mounts }
    }
}

#[cfg(test)]
impl DiskSource for MockSource {
    fn sample(&self) -> Result<Vec<Mount>> {
        Ok(self.mounts.clone())
    }
}

/// "Darwin 25.3.0" or similar. Used by `/v1/info`.
pub fn read_kernel_version() -> Option<String> {
    let kind = sysinfo::System::kernel_version();
    let osname = sysinfo::System::name();
    match (osname, kind) {
        (Some(n), Some(v)) => Some(format!("{n} {v}")),
        (None, Some(v)) => Some(v),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pseudo_filesystems_are_filtered() {
        assert!(!is_real_filesystem("/dev", "devfs", 1024));
        assert!(!is_real_filesystem("/etc/something", "autofs", 1024));
    }

    #[test]
    fn zero_byte_mounts_are_filtered() {
        assert!(!is_real_filesystem("/something", "apfs", 0));
    }

    #[test]
    fn root_apfs_is_kept() {
        assert!(is_real_filesystem("/", "apfs", 500_000_000_000));
    }

    #[test]
    fn external_volume_is_kept() {
        assert!(is_real_filesystem(
            "/Volumes/External",
            "exfat",
            2_000_000_000_000
        ));
    }

    #[test]
    fn hidden_system_volumes_are_filtered() {
        assert!(!is_real_filesystem(
            "/System/Volumes/Preboot",
            "apfs",
            500_000_000_000
        ));
        assert!(!is_real_filesystem(
            "/System/Volumes/VM",
            "apfs",
            500_000_000_000
        ));
        assert!(!is_real_filesystem(
            "/System/Volumes/Update/SFR/mnt1",
            "apfs",
            500_000_000_000
        ));
    }

    #[test]
    fn data_volume_is_kept() {
        // /System/Volumes/Data is the real user-data volume on Apple Silicon.
        // It does NOT live under any of the hidden prefixes; users may want it
        // surfaced when root differs from it.
        assert!(is_real_filesystem(
            "/System/Volumes/Data",
            "apfs",
            500_000_000_000
        ));
    }

    #[test]
    fn mock_source_returns_seeded_mounts() {
        let mount = Mount {
            mount_point: "/".into(),
            device: "/dev/disk3s1s1".into(),
            fs_type: "apfs".into(),
            usage: Usage {
                used_bytes: 50,
                free_bytes: 50,
                total_bytes: 100,
            },
            largest_files: Vec::new(),
            largest_files_scanned_at: None,
        };
        let mock = MockSource::new(vec![mount.clone()]);
        let sample = mock.sample().unwrap();
        assert_eq!(sample.len(), 1);
        assert_eq!(sample[0], mount);
    }
}
