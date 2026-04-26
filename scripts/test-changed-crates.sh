#!/usr/bin/env bash
set -euo pipefail

ROOT="${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
exec "${ROOT}/scripts/agent-gate" --staged "$@"
