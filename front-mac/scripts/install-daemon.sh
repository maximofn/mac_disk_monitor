#!/usr/bin/env bash
# Install / reinstall the LaunchAgent that runs the mac-disk-monitord backend
# on login (and restarts it via KeepAlive if it exits).
# Usage:
#   ./scripts/install-daemon.sh           # install + load
#   ./scripts/install-daemon.sh uninstall # unload + remove
set -euo pipefail

LABEL="com.maximofn.mac-disk-monitord"
SRC="$(cd "$(dirname "$0")" && pwd)/${LABEL}.plist"
DST="$HOME/Library/LaunchAgents/${LABEL}.plist"
BIN="$(cd "$(dirname "$0")/../.." && pwd)/target/release/mac-disk-monitord"

uid="$(id -u)"
domain="gui/${uid}"
target="${domain}/${LABEL}"

cmd="${1:-install}"

case "$cmd" in
    install)
        if [[ ! -x "$BIN" ]]; then
            echo "error: backend binary not found at: $BIN" >&2
            echo "       run 'cargo build --release --workspace' first." >&2
            exit 1
        fi

        if launchctl print "$target" >/dev/null 2>&1; then
            echo "==> bootout existing $LABEL"
            launchctl bootout "$target" || true
        fi

        echo "==> install $DST"
        mkdir -p "$HOME/Library/LaunchAgents" "$HOME/Library/Logs"
        cp "$SRC" "$DST"

        echo "==> bootstrap $target"
        launchctl bootstrap "$domain" "$DST"
        launchctl enable "$target"
        launchctl kickstart -k "$target"

        echo
        echo "Loaded. The backend will autostart on login and listen on http://127.0.0.1:9136."
        echo "Logs: ~/Library/Logs/mac-disk-monitord.{out,err}.log"
        ;;
    uninstall)
        if launchctl print "$target" >/dev/null 2>&1; then
            echo "==> bootout $target"
            launchctl bootout "$target" || true
        fi
        if [[ -f "$DST" ]]; then
            echo "==> remove $DST"
            rm -f "$DST"
        fi
        echo "Uninstalled."
        ;;
    *)
        echo "usage: $0 [install|uninstall]" >&2
        exit 2
        ;;
esac
