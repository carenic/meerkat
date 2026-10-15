#!/usr/bin/env bash
# Self-test for scripts/release-publish-rust-crates.sh, the level-parallel
# crates.io publisher, with a stubbed `cargo publish` and a file:// sparse
# index (no network, no upload). The real canonical order and `cargo metadata`
# supply the levels.
#
# It proves: every release crate is published exactly once; no crate starts
# before every crate of the levels below it finished and is in the index;
# uploads overlap within a level but never exceed MEERKAT_CRATE_PUBLISH_JOBS,
# while a dry run stays sequential and does not wait for the index; an
# already published version is skipped; the first failing crate stops the
# publication with its exit status before any later level starts; a crate
# that never reaches the index fails the level once
# MEERKAT_CRATE_INDEX_TIMEOUT has passed.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PUBLISHER="${REPO_ROOT}/scripts/release-publish-rust-crates.sh"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/meerkat-publish-levels.XXXXXX")"
trap '[[ -n "${KEEP_TEST_ROOT:-}" ]] || rm -rf "$TEST_ROOT"' EXIT

REAL_CARGO="$(command -v cargo)"
LEVELS="$("${REPO_ROOT}/scripts/release-rust-crates.sh" --levels)"
mkdir -p "${TEST_ROOT}/bin"

# cargo publish -p CRATE ...: records start/end times and the concurrency,
# then follows ${STATE}/behaviour/CRATE (default "ok"): ok uploads and adds
# the version to the index, exists answers like crates.io for a published
# version, fail exits 3, hidden uploads without ever reaching the index.
cat >"${TEST_ROOT}/bin/cargo" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" != "publish" ]]; then
  exec "${REAL_CARGO}" "$@"
fi
crate=""
dry_run=false
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    -p) crate="$2"; shift 2 ;;
    --dry-run) dry_run=true; shift ;;
    *) shift ;;
  esac
done
now_ms() { date +%s%3N; }
{
  flock 9
  running=$(( $(cat "${STATE}/running" 2>/dev/null || echo 0) + 1 ))
  echo "${running}" >"${STATE}/running"
  peak="$(cat "${STATE}/peak" 2>/dev/null || echo 0)"
  (( running > peak )) && echo "${running}" >"${STATE}/peak"
  echo "start ${crate} $(now_ms)" >>"${STATE}/events"
} 9>"${STATE}/lock"
sleep 0.1
behaviour="$(cat "${STATE}/behaviour/${crate}" 2>/dev/null || echo ok)"
status=0
case "${behaviour}" in
  ok|hidden)
    echo "    Uploaded ${crate} v${VERSION} to registry \`crates-io\`"
    if [[ "${behaviour}" == "ok" && "${dry_run}" == "false" ]]; then
      name="${crate,,}"
      path="${name:0:2}/${name:2:2}/${name}"
      mkdir -p "${INDEX_DIR}/$(dirname "${path}")"
      printf '{"name":"%s","vers":"%s"}\n' "${crate}" "${VERSION}" >>"${INDEX_DIR}/${path}"
    fi
    ;;
  exists)
    echo "error: crate version \`${VERSION}\` is already uploaded" >&2
    status=101
    ;;
  fail)
    echo "error: simulated upload failure" >&2
    status=3
    ;;
esac
{
  flock 9
  echo $(( $(cat "${STATE}/running") - 1 )) >"${STATE}/running"
  echo "end ${crate} $(now_ms)" >>"${STATE}/events"
} 9>"${STATE}/lock"
exit "${status}"
STUB
chmod +x "${TEST_ROOT}/bin/cargo"

fail() {
  echo "level-parallel publisher contract violated: $1" >&2
  shift
  for extra in "$@"; do
    echo "  ${extra}" >&2
  done
  exit 1
}

VERSION="$(awk -F '\t' 'NR == 1 { print $3 }' <<<"${LEVELS}")"
export REAL_CARGO VERSION

# publish <label> <expected status> [VAR=value ...]: runs the publisher.
publish() {
  local label="$1" expected="$2"
  shift 2
  STATE="${TEST_ROOT}/${label}"
  INDEX_DIR="${STATE}/index"
  mkdir -p "${STATE}/behaviour" "${INDEX_DIR}" "${STATE}/runner-temp"
  : >"${STATE}/events"
  export STATE INDEX_DIR
  local status
  set +e
  env PATH="${TEST_ROOT}/bin:${PATH}" ROOT="${REPO_ROOT}" RUNNER_TEMP="${STATE}/runner-temp" \
    GITHUB_ACTIONS=false MEERKAT_CRATE_INDEX_URL="file://${INDEX_DIR}" \
    MEERKAT_CRATE_INDEX_POLL_SECONDS=0 "$@" \
    "${PUBLISHER}" >"${STATE}/out.log" 2>&1
  status=$?
  set -e
  if [[ "${status}" -ne "${expected}" ]]; then
    fail "${label}: expected exit ${expected}, got ${status}" "$(tail -n 20 "${STATE}/out.log")"
  fi
}

# prepare <label> <crate> <behaviour>: set one crate's behaviour before a run.
prepare() {
  mkdir -p "${TEST_ROOT}/$1/behaviour"
  echo "$3" >"${TEST_ROOT}/$1/behaviour/$2"
}

# Every start of a level-N crate is after every end of every lower level.
check_level_order() {
  local label="$1"
  python3 - "${TEST_ROOT}/${label}/events" "${LEVELS}" <<'PY' || fail "${label}: a level started before a lower level finished"
import sys
events_path, levels_tsv = sys.argv[1:3]
level = {}
for line in levels_tsv.splitlines():
    number, crate, _ = line.split("\t")
    level[crate] = int(number)
start, end = {}, {}
for line in open(events_path, encoding="utf-8"):
    kind, crate, stamp = line.split()
    (start if kind == "start" else end)[crate] = int(stamp)
for crate, began in start.items():
    for other, finished in end.items():
        if level[other] < level[crate] and finished > began:
            sys.exit(f"{crate} (level {level[crate]}) started before {other} (level {level[other]}) finished")
PY
}

started() {
  awk '$1 == "start" { print $2 }' "${TEST_ROOT}/$1/events" | sort
}

all_crates="$(cut -f2 <<<"${LEVELS}" | sort)"

# 1. Uploads: every crate once, levels in order, overlap bounded by the jobs,
#    and an already published version (the first crate) skipped.
first_crate="$(awk -F '\t' 'NR == 1 { print $2 }' <<<"${LEVELS}")"
prepare uploads "${first_crate}" exists
name="${first_crate,,}"
mkdir -p "${TEST_ROOT}/uploads/index/${name:0:2}/${name:2:2}"
printf '{"name":"%s","vers":"%s"}\n' "${first_crate}" "${VERSION}" >"${TEST_ROOT}/uploads/index/${name:0:2}/${name:2:2}/${name}"
publish uploads 0 MEERKAT_CRATE_PUBLISH_JOBS=4
[[ "$(started uploads)" == "${all_crates}" ]] || fail "uploads: not every crate was published exactly once"
check_level_order uploads
peak="$(cat "${TEST_ROOT}/uploads/peak")"
if [[ "${peak}" -lt 2 || "${peak}" -gt 4 ]]; then
  fail "uploads: peak concurrency ${peak}, expected between 2 and MEERKAT_CRATE_PUBLISH_JOBS=4"
fi
grep -Fq "${first_crate}: already published, skipping" "${TEST_ROOT}/uploads/out.log" ||
  fail "uploads: the published version was not reported as skipped"
grep -Fq "Every release crate is published" "${TEST_ROOT}/uploads/out.log" || fail "uploads: no completion line"
echo "ok: uploads publish every crate once, level by level, at most 4 at a time (peak ${peak}), skipping a published version"

# 2. A dry run checks one crate at a time and never waits for the index (the
#    stub adds no entry for a dry run); a failure stops it as it stops uploads.
dry_level=2
dry_crate="$(awk -F '\t' -v level="${dry_level}" '$1 == level { print $2; exit }' <<<"${LEVELS}")"
prepare dry-run "${dry_crate}" fail
publish dry-run 3 MEERKAT_REGISTRY_DRY_RUN=true MEERKAT_CRATE_INDEX_TIMEOUT=1
[[ "$(cat "${TEST_ROOT}/dry-run/peak")" -eq 1 ]] || fail "dry-run: crates overlapped"
check_level_order dry-run
below="$(awk -F '\t' -v level="${dry_level}" '$1 < level { print $2 }' <<<"${LEVELS}" | sort)"
[[ "$(comm -12 <(started dry-run) <(printf '%s\n' "${below}"))" == "${below}" ]] ||
  fail "dry-run: the levels below ${dry_crate} did not all run without index entries"
if awk -F '\t' -v level="${dry_level}" '$1 > level { print $2 }' <<<"${LEVELS}" | grep -Fxq -f - <(started dry-run); then
  fail "dry-run: a later level started after ${dry_crate} failed"
fi
echo "ok: a dry run checks crates one at a time without waiting for the index and stops at ${dry_crate}"

# 4. A failing crate stops the publication with its status: once it failed,
#    no further crate of its level starts (one at a time here, so none of its
#    siblings), and no later level starts.
failing_level=3
failing_crate="$(awk -F '\t' -v level="${failing_level}" '$1 == level { print $2; exit }' <<<"${LEVELS}")"
prepare failure "${failing_crate}" fail
publish failure 3 MEERKAT_CRATE_PUBLISH_JOBS=1
check_level_order failure
later="$(awk -F '\t' -v level="${failing_level}" '$1 > level { print $2 }' <<<"${LEVELS}" | sort)"
if comm -12 <(started failure) <(printf '%s\n' "${later}") | grep -q .; then
  fail "failure: a crate of a later level started after ${failing_crate} failed"
fi
siblings="$(awk -F '\t' -v level="${failing_level}" -v crate="${failing_crate}" '$1 == level && $2 != crate { print $2 }' <<<"${LEVELS}" | sort)"
if comm -12 <(started failure) <(printf '%s\n' "${siblings}") | grep -q .; then
  fail "failure: a sibling of ${failing_crate} started after it failed" "$(comm -12 <(started failure) <(printf '%s\n' "${siblings}"))"
fi
grep -Fq "Level ${failing_level}: ${failing_crate} failed (exit 3)" "${TEST_ROOT}/failure/out.log" ||
  fail "failure: the failing crate is not named"
grep -Fq "and the rest of the level are not started" "${TEST_ROOT}/failure/out.log" ||
  fail "failure: the unstarted rest of the level is not reported"
echo "ok: ${failing_crate} failing stops the publication with exit 3; no further crate starts"

# 5. A crate that never reaches the index fails its level after the timeout.
hidden_crate="$(awk -F '\t' 'NR == 1 { print $2 }' <<<"${LEVELS}")"
prepare invisible "${hidden_crate}" hidden
publish invisible 1 MEERKAT_CRATE_INDEX_TIMEOUT=1
grep -Fq "Level 0: not visible in" "${TEST_ROOT}/invisible/out.log" ||
  fail "invisible: the missing index entry is not named" "$(tail -n 5 "${TEST_ROOT}/invisible/out.log")"
if awk -F '\t' '$1 > 0 { print $2 }' <<<"${LEVELS}" | grep -Fxq -f - <(started invisible); then
  fail "invisible: level 1 started although level 0 is not in the index"
fi
echo "ok: a crate missing from the index fails its level after MEERKAT_CRATE_INDEX_TIMEOUT"

echo "level-parallel publisher contract holds"
