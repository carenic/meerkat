#!/usr/bin/env bash
# Verify router, RPC catalog, and docs method inventory remain aligned.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

green() { printf '\033[0;32m%s\033[0m\n' "$*"; }

python3 "$ROOT/scripts/verify_rpc_surface_alignment.py" "$ROOT"

green "RPC surface alignment check passed"
