# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project layout

Two halves living side by side, sharing nothing at runtime except the JSON wire-format:

- **Rust backend** in `crates/` (Cargo workspace): `mac-disk-monitor-core` (shared serde types) + `mac-disk-monitord` (HTTP/SSE daemon, default `127.0.0.1:9136`).
- **Swift frontend** in `front-mac/` (Swift Package, AppKit, no third-party deps): a menubar-only app (`LSUIElement`) that consumes the daemon's `/v1/stream`.

The on-the-wire schema (`crates/mac-disk-monitor-core/src/model.rs` ↔ `front-mac/Sources/MacDiskMonitorTray/Models.swift`) is intentionally identical to the Linux sibling at `../disk_monitor`. **If you add or rename a field on the Rust side, mirror it in `Models.swift` (with the matching `CodingKeys`) or the JSON decode silently drops it.**

## Common commands

Rust requires `rustup` (≥ 1.85). It was installed with `--no-modify-path`, so prefix Rust commands with `. "$HOME/.cargo/env"` if `cargo` isn't already on `PATH`.

```bash
# Rust (run from repo root)
cargo build --workspace                 # debug
cargo build --release --workspace       # release → target/release/mac-disk-monitord
cargo test --workspace                  # core has a JSON-roundtrip test; daemon covers source filters + cache
cargo clippy --workspace --all-targets

# Swift (run from front-mac/)
./scripts/build-app.sh                  # → build/Mac Disk Monitor.app
swift build -c release --arch arm64     # raw binary only, no .app wrapper

# End-to-end
./target/release/mac-disk-monitord --port 9136 &
open "front-mac/build/Mac Disk Monitor.app"
curl -s http://127.0.0.1:9136/v1/snapshot | jq
curl -N http://127.0.0.1:9136/v1/stream      # SSE, one event per second
"front-mac/build/Mac Disk Monitor.app/Contents/MacOS/mac-disk-monitor-tray" --dump-icon /tmp/icon.png
```

## Architecture notes that span files

### Sampling thread model (Rust)

Unlike the CPU/RAM siblings, `mac-disk-monitord` does **not** spawn a dedicated `std::thread` for sampling. The disk sample is cheap (one `getfsstat`-equivalent call via `sysinfo::Disks::new_with_refreshed_list()`) so `sampler::spawn` runs on a normal `tokio::spawn` driven by `tokio::time::interval`.

The largest-files scanner does the heavy work and runs on `tokio::task::spawn_blocking` because `walkdir` is sync — see `largest_files.rs::scan_one`.

### Mount filtering

`source.rs::is_real_filesystem` drops:

1. Pseudo filesystems by `fs_type`: `devfs`, `autofs`, `ctl`, `fdesc`, `tmpfs`, `msdos` (EFI System Partition).
2. Hidden APFS system slices by mount-point prefix: `/System/Volumes/Preboot`, `/System/Volumes/VM`, `/System/Volumes/Update*`, `/System/Volumes/iSCPreboot`, `/System/Volumes/xarts`, `/System/Volumes/Hardware`, `/System/Volumes/Recovery`, `/private/var/vm`.
3. Zero-byte mounts (uninitialised volumes).

After filtering, mounts are also de-duped by `device` name — APFS containers report the same backing device across multiple "volumes", and the root mount's usage is the canonical figure for the container.

`/System/Volumes/Data` (the real user-data volume on Apple Silicon) is *kept* because its prefix doesn't match the hidden list — but in practice it's already de-duped against `/` via the device-name filter. If you ever need to surface it separately, drop it from the dedup set.

### HTTP/SSE surface

`crates/mac-disk-monitord/src/http/mod.rs` wires the routes; routes only ever read the latest `Snapshot` from the `watch::Receiver` clone in `AppState`. SSE (`http/sse.rs`) wraps that receiver in `tokio_stream::wrappers::WatchStream` so each new snapshot becomes one SSE event automatically — there is no per-client buffering or sample loop on the HTTP side.

Endpoints: `/healthz`, `/v1/info`, `/v1/snapshot`, `/v1/mounts`, `/v1/mounts/{path}`, `POST /v1/rescan`, `POST /v1/rescan/{path}`, `/v1/stream`. Defaults to `127.0.0.1:9136`. The port assignment is deliberate: Linux variants use the 9123-9126 band (cpu=9124, gpu=9123, ram=9125, disk=9126); Mac variants use 9133-9136 with the same trailing digit (cpu=9134, gpu=9133, ram=9135, disk=9136). That way a single Mac can simultaneously run its own backends and SSH-tunnel the Linux siblings.

### Largest-files scanner

Off by default on macOS. Reason: most user folders (Documents, Desktop, Downloads, the privacy-protected `Library/...` paths) are TCC-protected; without Full Disk Access the walker silently skips them, producing a useless top-N. Pass `--no-largest-files=false` (or `MAC_DISK_MONITORD_NO_LARGEST_FILES=false`) once Full Disk Access has been granted via System Settings → Privacy & Security.

The scanner stays within one filesystem (`WalkDir::same_file_system(true)`), does not follow symlinks, and de-duplicates hardlinks via `(dev, inode)`. Same algorithm as the Linux sibling.

### Swift menubar app

`StatusBarController.refreshIcon` dedupes via a render key (`mountPoint:pct|connected|appearance`) so identical 1-Hz ticks don't repaint. Light/dark switching listens on `AppleInterfaceThemeChangedNotification` via `DistributedNotificationCenter` — **don't KVO `effectiveAppearance` on the status item button**, the comment in that file explains the feedback loop that caused.

`SSEClient` (`Client.swift`) parses SSE manually because `Foundation.AsyncBytes.lines` collapses the blank-line frame separators; it decodes a `Snapshot` after every `data:` line on the assumption that `mac-disk-monitord` ships one self-contained JSON snapshot per event (which it does — see `http/sse.rs`).

`IconRenderer` is adapted from the Linux disk_monitor's `front-mac/` (which uses Core Graphics + Core Text). Each mount renders as `[icon] [short label] [donut with %]` — multiple mounts render side by side with a 4 pt gap. The `disk.png` base icon is loaded via `Bundle.module`; `build-app.sh` copies the SwiftPM-generated resource bundle next to the binary inside the `.app/Contents/MacOS/` so `Bundle.module` resolves at runtime.

### Autostart

`front-mac/scripts/install-daemon.sh` and `install-launchagent.sh` install two LaunchAgents under `~/Library/LaunchAgents/`. The plists hardcode the absolute path to `target/release/mac-disk-monitord` and to the bundled `.app`; if the project moves on disk, regenerate them or run the install scripts again.

## When changing the schema

1. Edit `crates/mac-disk-monitor-core/src/model.rs`.
2. Mirror in `front-mac/Sources/MacDiskMonitorTray/Models.swift` — same field order, matching `CodingKeys` for the snake_case ↔ camelCase mapping.
3. Rebuild both halves: `cargo build --workspace` and `./front-mac/scripts/build-app.sh`.
4. Smoke test: `curl -s http://127.0.0.1:9136/v1/snapshot | jq` to confirm new fields serialise; the Swift side will silently ignore unknown JSON keys, so the failure mode is "field stays `nil`/`zero`" — easy to miss without an end-to-end check.

The same schema is used by the Linux `disk-monitord` at `../disk_monitor`; keep them in sync if the change is supposed to be cross-platform (e.g. so a single Home Assistant package works against both backends).
