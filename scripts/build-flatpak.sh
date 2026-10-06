#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

flatpak run org.flatpak.Builder \
    --user \
    --install \
    --force-clean \
    --install-deps-from=flathub \
    build-dir \
    flatpak/io.github.Mars7x.Horizon.yml
