#!/usr/bin/env bash
set -euo pipefail

ROOT="${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
exec "${ROOT}/scripts/cargo-agent-gate" --staged "$@"
