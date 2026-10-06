#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GENERATOR="${FLATPAK_CARGO_GENERATOR:-flatpak-cargo-generator.py}"

cd "$ROOT"
cargo generate-lockfile
python3 "$GENERATOR" Cargo.lock -o flatpak/cargo-sources.json

echo "Updated flatpak/cargo-sources.json"
