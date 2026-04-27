#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

cd "$ROOT"

CARGO="${CARGO:-./scripts/repo-cargo}"

exec "$CARGO" xtask machine-verify --all "$@"
