use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use mac_disk_monitor_core::{FileEntry, Snapshot};
use tokio::sync::{mpsc, watch};
use walkdir::WalkDir;

#[derive(Clone, Debug)]
pub struct ScanResult {
    pub files: Vec<FileEntry>,
    pub scanned_at: String,
}

/// In-memory cache mapping mount_point → most recent scan result. Sampler
/// reads it when assembling each snapshot; scanner writes it after every
/// full walk completes.
#[derive(Clone, Default)]
pub struct LargestFilesCache {
    inner: Arc<Mutex<HashMap<String, ScanResult>>>,
}

impl LargestFilesCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, mount_point: &str) -> Option<ScanResult> {
        self.inner.lock().unwrap().get(mount_point).cloned()
    }

    pub fn put(&self, mount_point: String, result: ScanResult) {
        self.inner.lock().unwrap().insert(mount_point, result);
    }

    pub fn retain(&self, keep: &[String]) {
        let mut g = self.inner.lock().unwrap();
        g.retain(|k, _| keep.contains(k));
    }
}

#[derive(Clone, Debug)]
pub enum RescanRequest {
    All,
    One(String),
}

pub type RescanTrigger = mpsc::UnboundedSender<RescanRequest>;

#[derive(Clone, Debug)]
pub struct ScannerConfig {
    pub top_n: usize,
    pub refresh_interval: Duration,
    pub initial_delay: Duration,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        Self {
            top_n: 20,
            refresh_interval: Duration::from_secs(600),
            initial_delay: Duration::from_secs(60),
        }
    }
}

/// Spawn the background largest-files scanner. macOS-specific note: walks
/// will hit TCC-protected directories (Documents, Desktop, Downloads, the
/// `Library/...` privacy bins). Without Full Disk Access they error with
/// EPERM and are silently skipped — same behavior `du` exhibits.
pub fn spawn(
    cfg: ScannerConfig,
    cache: LargestFilesCache,
    snapshot_rx: watch::Receiver<Snapshot>,
) -> RescanTrigger {
    let (trigger_tx, mut trigger_rx) = mpsc::unbounded_channel::<RescanRequest>();

    tokio::spawn(async move {
        tokio::time::sleep(cfg.initial_delay).await;
        scan_all(&cfg, &cache, &snapshot_rx).await;

        loop {
            tokio::select! {
                _ = tokio::time::sleep(cfg.refresh_interval) => {
                    scan_all(&cfg, &cache, &snapshot_rx).await;
                }
                Some(req) = trigger_rx.recv() => {
                    let mut targets: HashSet<String> = HashSet::new();
                    let mut all = false;
                    let handle = |r: RescanRequest, targets: &mut HashSet<String>, all: &mut bool| match r {
                        RescanRequest::All => *all = true,
                        RescanRequest::One(mp) => { targets.insert(mp); }
                    };
                    handle(req, &mut targets, &mut all);
                    while let Ok(extra) = trigger_rx.try_recv() {
                        handle(extra, &mut targets, &mut all);
                    }
                    if all {
                        scan_all(&cfg, &cache, &snapshot_rx).await;
                    } else {
                        for mp in targets {
                            scan_one(&cfg, &cache, &mp).await;
                        }
                    }
                }
            }
        }
    });

    trigger_tx
}

async fn scan_all(cfg: &ScannerConfig, cache: &LargestFilesCache, rx: &watch::Receiver<Snapshot>) {
    let mounts: Vec<String> = rx
        .borrow()
        .mounts
        .iter()
        .map(|m| m.mount_point.clone())
        .collect();
    cache.retain(&mounts);
    for mp in mounts {
        scan_one(cfg, cache, &mp).await;
    }
}

async fn scan_one(cfg: &ScannerConfig, cache: &LargestFilesCache, mount_point: &str) {
    let cache = cache.clone();
    let mp = mount_point.to_string();
    let top_n = cfg.top_n;
    let result = tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let files = scan_mount(Path::new(&mp), top_n);
        let elapsed = started.elapsed();
        tracing::info!(
            mount = %mp,
            files = files.len(),
            elapsed_ms = elapsed.as_millis() as u64,
            "largest-files scan complete"
        );
        ScanResult {
            files,
            scanned_at: Utc::now().to_rfc3339(),
        }
    })
    .await;
    match result {
        Ok(res) => cache.put(mount_point.to_string(), res),
        Err(err) => tracing::warn!(mount = %mount_point, error = %err, "scan task panicked"),
    }
}

/// Walk `mount_point` and return the `top_n` largest regular files.
///
/// - Stays within one filesystem (does not descend into nested mounts).
/// - Skips symlinks (does not follow them).
/// - Dedupes hardlinks via (dev, inode) so a file with N hardlinks counts once.
/// - Silently swallows permission and read errors (matches what `du` does
///   when run unprivileged on macOS without Full Disk Access).
pub fn scan_mount(mount_point: &Path, top_n: usize) -> Vec<FileEntry> {
    if top_n == 0 {
        return Vec::new();
    }
    let mut heap: BinaryHeap<Reverse<(u64, PathBuf)>> = BinaryHeap::with_capacity(top_n + 1);
    let mut seen_inodes: HashSet<(u64, u64)> = HashSet::new();

    let walker = WalkDir::new(mount_point)
        .same_file_system(true)
        .follow_links(false);

    for entry in walker.into_iter().filter_map(|r| r.ok()) {
        let ft = entry.file_type();
        if !ft.is_file() {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.nlink() > 1 && !seen_inodes.insert((meta.dev(), meta.ino())) {
            continue;
        }
        let size = meta.len();
        if heap.len() < top_n {
            heap.push(Reverse((size, entry.path().to_path_buf())));
        } else if let Some(Reverse((min_size, _))) = heap.peek() {
            if size > *min_size {
                heap.pop();
                heap.push(Reverse((size, entry.path().to_path_buf())));
            }
        }
    }

    let mut out: Vec<(u64, PathBuf)> = heap.into_iter().map(|r| r.0).collect();
    out.sort_by_key(|p| Reverse(p.0));
    out.into_iter()
        .map(|(size, path)| FileEntry {
            path: path.to_string_lossy().into_owned(),
            size_bytes: size,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn scan_returns_top_n_in_size_order() {
        let dir = tempdir().unwrap();
        for (name, size) in [("a", 100u64), ("b", 5_000), ("c", 1_000), ("d", 50)] {
            let mut f = fs::File::create(dir.path().join(name)).unwrap();
            f.write_all(&vec![0u8; size as usize]).unwrap();
        }
        let result = scan_mount(dir.path(), 2);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].size_bytes, 5_000);
        assert!(result[0].path.ends_with("/b"));
        assert_eq!(result[1].size_bytes, 1_000);
        assert!(result[1].path.ends_with("/c"));
    }

    #[test]
    fn cache_round_trip_and_retain() {
        let cache = LargestFilesCache::new();
        cache.put(
            "/Volumes/A".into(),
            ScanResult {
                files: vec![],
                scanned_at: "now".into(),
            },
        );
        cache.put(
            "/".into(),
            ScanResult {
                files: vec![],
                scanned_at: "now".into(),
            },
        );
        assert!(cache.get("/Volumes/A").is_some());
        cache.retain(&["/".into()]);
        assert!(cache.get("/Volumes/A").is_none());
        assert!(cache.get("/").is_some());
    }
}
