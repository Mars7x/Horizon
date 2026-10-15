#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GENERATOR="${FLATPAK_CARGO_GENERATOR:-flatpak-cargo-generator.py}"

cd "$ROOT"
# Validate the existing lockfile; source generation must not upgrade dependencies.
cargo metadata --locked --no-deps --format-version 1 >/dev/null
python3 "$GENERATOR" Cargo.lock -o flatpak/cargo-sources.json

echo "Updated flatpak/cargo-sources.json"
