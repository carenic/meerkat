#!/usr/bin/env bash
# Verify canonical app-facing RPC methods from the catalog are represented in
# both SDK source trees (TypeScript + Python).

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

green() { printf '\033[0;32m%s\033[0m\n' "$*"; }

python3 "$ROOT/scripts/verify_sdk_wrapper_freshness.py" "$ROOT"

green "SDK wrapper freshness check passed"
