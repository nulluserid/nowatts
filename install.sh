#!/usr/bin/env bash
# nowatts installer — daemon binary + systemd --user unit + Omarchy shell widget.
#
# Read-only daemon (FR-001): never touches sysfs, runs unprivileged.
# Safe to re-run: overwrites its own files only, then restarts the service
# and rescans the shell plugins.
set -euo pipefail
cd "$(dirname "$0")"

BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
UNIT_DIR="${UNIT_DIR:-$HOME/.config/systemd/user}"
PLUGIN_DIR="${PLUGIN_DIR:-$HOME/.config/omarchy/plugins/legend.nowatts}"

command -v cargo >/dev/null || { echo "cargo not found (mise: \`mise use rust\`)" >&2; exit 1; }
command -v systemctl >/dev/null || { echo "systemctl not found" >&2; exit 1; }

echo "==> building (release)"
cargo build --release --manifest-path nowatts/Cargo.toml

echo "==> installing daemon binary to $BIN_DIR/nowatts"
mkdir -p "$BIN_DIR"
# Stop first: a running binary cannot be overwritten (ETXTBSY).
systemctl --user stop nowatts 2>/dev/null || true
cp nowatts/target/release/nowatts "$BIN_DIR/nowatts"

echo "==> installing unit to $UNIT_DIR/nowatts.service"
mkdir -p "$UNIT_DIR"
cp nowatts.service "$UNIT_DIR/nowatts.service"
systemctl --user daemon-reload
systemctl --user enable --now nowatts

echo "==> installing widget to $PLUGIN_DIR"
mkdir -p "$(dirname "$PLUGIN_DIR")"
rm -rf "$PLUGIN_DIR"
cp -r plugin "$PLUGIN_DIR"
omarchy plugin enable legend.nowatts 2>/dev/null || true
# QML component cache keeps stale builds until the shell restarts
# (observed on Omarchy 4.0.4 — see ledger); restart once so the fresh
# widget code is what renders.
omarchy restart shell

echo "==> done."
echo "    bar widget: right-click settings · panel via click or:"
echo "    omarchy-shell nowatts toggle"
echo "    logs: journalctl --user -u nowatts -f"
