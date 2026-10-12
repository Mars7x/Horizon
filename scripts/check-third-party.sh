#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
notice="$root/THIRD_PARTY_NOTICES.md"

required_assets=(
  "controllers/controller_xbox360.svg"
  "controllers/controller_xboxseries.svg"
  "controllers/controller_playstation3.svg"
  "controllers/controller_playstation4.svg"
  "controllers/controller_playstation5.svg"
  "controllers/controller_switch_pro.svg"
  "controllers/switch_joycon_left.svg"
  "controllers/switch_joycon_right.svg"
  "controllers/switch_joycon.svg"
  "controllers/controller_steamdeck.svg"
  "controllers/controller_generic.svg"
)

required_licenses=(
  "CC0-1.0.txt"
  "CC-BY-SA-3.0-US.txt"
  "GPL-3.0-or-later.txt"
  "OFL-1.1.txt"
  "Apache-2.0.txt"
  "MIT.txt"
)

for asset in "${required_assets[@]}"; do
  test -f "$root/ui/assets/$asset" || { echo "missing third-party asset: $asset" >&2; exit 1; }
  grep -Fq "$asset" "$notice" || { echo "asset missing from THIRD_PARTY_NOTICES.md: $asset" >&2; exit 1; }
done

grep -Fq "LINE Seed JP" "$notice" || { echo "LINE Seed JP missing from THIRD_PARTY_NOTICES.md" >&2; exit 1; }
grep -Fq "© LY Corporation" "$notice" || { echo "LINE Seed JP copyright missing from THIRD_PARTY_NOTICES.md" >&2; exit 1; }
grep -Fq "rusqlite / libsqlite3-sys" "$notice" || { echo "rusqlite provenance missing from THIRD_PARTY_NOTICES.md" >&2; exit 1; }
grep -Fq "SQLite core" "$notice" || { echo "SQLite provenance missing from THIRD_PARTY_NOTICES.md" >&2; exit 1; }
grep -Fq "steam-vdf-parser 0.1.2" "$notice" || { echo "steam-vdf-parser provenance missing from THIRD_PARTY_NOTICES.md" >&2; exit 1; }
grep -Fq "serde 1.x / serde_json 1.x" "$notice" || { echo "Serde provenance missing from THIRD_PARTY_NOTICES.md" >&2; exit 1; }
# The retired Bottles adapter no longer uses yaml_serde. Do not require a
# phantom dependency notice for a crate absent from Cargo.toml/Cargo.lock.
grep -Fq "zbus 5.x" "$notice" || { echo "zbus provenance missing from THIRD_PARTY_NOTICES.md" >&2; exit 1; }
grep -Fq "**FFmpeg**" "$notice" || { echo "FFmpeg provenance missing from THIRD_PARTY_NOTICES.md" >&2; exit 1; }
grep -Fq "**PipeWire**" "$notice" || { echo "PipeWire provenance missing from THIRD_PARTY_NOTICES.md" >&2; exit 1; }
grep -Fq "**jxl-oxide 0.12.x**" "$notice" || { echo "jxl-oxide provenance missing from THIRD_PARTY_NOTICES.md" >&2; exit 1; }

for license in "${required_licenses[@]}"; do
  test -s "$root/LICENSES/$license" || { echo "missing license text: $license" >&2; exit 1; }
done

echo "third-party attribution checks passed"
