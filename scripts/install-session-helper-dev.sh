#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${1:-$root/target/release/horizon-session-helper}"
install_root="${HOME}/.local/libexec"
unit_root="${HOME}/.config/systemd/user"

if [[ ! -x "$binary" ]]; then
  cat >&2 <<EOF
host session/runtime helper binary not found or not executable:
  $binary

Build it on the host/toolbox first:
  cargo build --release --bin horizon-session-helper

Or pass an explicit built binary path:
  $0 /path/to/horizon-session-helper
EOF
  exit 1
fi

install -d "$install_root" "$unit_root"
# Stop an older helper before replacing its executable. This matters when a
# protocol update is being installed over an already-running user service.
systemctl --user stop horizon-session-helper.service 2>/dev/null || true
install -m0755 "$binary" "$install_root/horizon-session-helper"
install -m0644 "$root/data/horizon-session-helper.service" \
  "$unit_root/horizon-session-helper.service"

systemctl --user daemon-reload
systemctl --user enable horizon-session-helper.service
systemctl --user restart horizon-session-helper.service

echo "installed Horizon managed-session/runtime helper"
systemctl --user --no-pager --full status horizon-session-helper.service || true
