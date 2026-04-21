#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <main-worktree> <feature-worktree>" >&2
  exit 2
fi

main_worktree="$1"
feature_worktree="$2"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

run_verify() {
  local worktree="$1"
  local logfile="$2"
  (
    cd "$worktree"
    make machine-verify 2>&1 | tee "$logfile"
  )
}

extract_stats() {
  local logfile="$1"
  local jsonfile="$2"
  python - "$logfile" "$jsonfile" <<'PY'
import json
import re
import sys
from pathlib import Path

log_path = Path(sys.argv[1])
json_path = Path(sys.argv[2])
lines = log_path.read_text().splitlines()

target_re = re.compile(r"^(Verifying machine|Verifying composition)\s+(.+?)\s*$")
states_re = re.compile(r"(\d+)\s+states generated,\s+(\d+)\s+distinct states found")
depth_re = re.compile(r"The depth of the complete state graph search is\s+(\d+)\.?")

items = []
current = None
for raw in lines:
    line = raw.strip()
    m = target_re.match(line)
    if m:
        if current is not None:
            items.append(current)
        current = {
            "kind": "machine" if m.group(1) == "Verifying machine" else "composition",
            "target": m.group(2),
            "generated_states": None,
            "distinct_states": None,
            "depth": None,
        }
        continue
    if current is None:
        continue
    m = states_re.search(line)
    if m:
        current["generated_states"] = int(m.group(1))
        current["distinct_states"] = int(m.group(2))
        continue
    m = depth_re.search(line)
    if m:
        current["depth"] = int(m.group(1))

if current is not None:
    items.append(current)

json_path.write_text(json.dumps(items, indent=2, sort_keys=True) + "\n")
PY
}

run_verify "$main_worktree" "$tmpdir/main.log"
run_verify "$feature_worktree" "$tmpdir/feature.log"

extract_stats "$tmpdir/main.log" "$tmpdir/main.json"
extract_stats "$tmpdir/feature.log" "$tmpdir/feature.json"

if ! diff -u "$tmpdir/main.json" "$tmpdir/feature.json"; then
  echo "machine-verify stats drift detected" >&2
  exit 1
fi

cat "$tmpdir/feature.json"
