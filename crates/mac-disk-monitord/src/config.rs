use std::net::IpAddr;

use clap::Parser;
use mac_disk_monitor_core::{DEFAULT_BIND, DEFAULT_PORT};

#[derive(Debug, Clone, Parser)]
#[command(name = "mac-disk-monitord", about = "macOS disk monitor backend daemon", version)]
pub struct Config {
    #[arg(long, env = "MAC_DISK_MONITORD_BIND", default_value = DEFAULT_BIND)]
    pub bind: IpAddr,

    #[arg(long, env = "MAC_DISK_MONITORD_PORT", default_value_t = DEFAULT_PORT)]
    pub port: u16,

    /// Sampling cadence in milliseconds. Disk usage doesn't move second-by-second,
    /// but matching the sibling daemons at 1 Hz keeps the SSE feed identical.
    #[arg(long, env = "MAC_DISK_MONITORD_SAMPLE_INTERVAL_MS", default_value_t = 1000)]
    pub sample_interval_ms: u64,

    #[arg(long, env = "RUST_LOG", default_value = "info")]
    pub log_level: String,

    /// How many largest files to keep per mount.
    #[arg(long, env = "MAC_DISK_MONITORD_LARGEST_TOP_N", default_value_t = 20)]
    pub largest_top_n: usize,

    /// Seconds between full largest-files re-scans. Re-scans are also
    /// triggered on demand via `POST /v1/rescan` regardless of this interval.
    #[arg(long, env = "MAC_DISK_MONITORD_LARGEST_REFRESH_SECS", default_value_t = 600)]
    pub largest_refresh_secs: u64,

    /// Seconds to wait before the first largest-files scan kicks in. macOS
    /// TCC prompts for several protected directories (Documents, Desktop,
    /// Downloads, …) on first access, so giving the user time to settle in
    /// avoids a flurry of dialogs at login.
    #[arg(long, env = "MAC_DISK_MONITORD_LARGEST_INITIAL_DELAY_SECS", default_value_t = 60)]
    pub largest_initial_delay_secs: u64,

    /// Disable the largest-files background scanner entirely. The default on
    /// macOS until you've granted Full Disk Access — otherwise the scan trips
    /// TCC and most user folders stay invisible to the daemon.
    #[arg(long, env = "MAC_DISK_MONITORD_NO_LARGEST_FILES", default_value_t = true)]
    pub no_largest_files: bool,
}
