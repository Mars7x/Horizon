#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
notice="$root/THIRD_PARTY_NOTICES.md"

required_assets=(
  "applications-games-symbolic.svg"
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

for license in "${required_licenses[@]}"; do
  test -s "$root/LICENSES/$license" || { echo "missing license text: $license" >&2; exit 1; }
done

echo "third-party attribution checks passed"
