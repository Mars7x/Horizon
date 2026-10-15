#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

python3 scripts/check-repository.py
python3 scripts/test-repository-checks.py
python3 scripts/check-retirement-migrations.py
bash scripts/check-third-party.sh
