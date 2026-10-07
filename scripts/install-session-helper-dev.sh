#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${1:-$root/target/release/horizon-session-helper}"
install_root="${HOME}/.local/libexec"
unit_root="${HOME}/.config/systemd/user"

if [[ ! -x "$binary" ]]; then
  cat >&2 <<EOF
managed-session helper binary not found or not executable:
  $binary

Build it on the host/toolbox first:
  cargo build --release --bin horizon-session-helper

Or pass an explicit built binary path:
  $0 /path/to/horizon-session-helper
EOF
  exit 1
fi

install -d "$install_root" "$unit_root"
install -m0755 "$binary" "$install_root/horizon-session-helper"
install -m0644 "$root/data/horizon-session-helper.service" \
  "$unit_root/horizon-session-helper.service"

systemctl --user daemon-reload
systemctl --user enable --now horizon-session-helper.service

echo "installed Horizon managed-session helper"
systemctl --user --no-pager --full status horizon-session-helper.service || true
