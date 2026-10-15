#!/usr/bin/env bash
# Publish every release crate to crates.io, one dependency level at a time.
#
# The levels come from the canonical order (scripts/release-rust-crates.sh
# --levels: that order plus `cargo metadata`). The crates of one level do not
# depend on each other, so they publish concurrently, at most
# MEERKAT_CRATE_PUBLISH_JOBS at a time (default 6 for uploads, which skip
# cargo's verify build; 1 for a dry run or a verifying upload, which compile
# each crate in its own target directory). The next level starts only after
# every crate of this one is visible in the crates.io sparse index
# (MEERKAT_CRATE_INDEX_URL, default https://index.crates.io), for at most
# MEERKAT_CRATE_INDEX_TIMEOUT seconds per level (default 600).
#
# Failure semantics are those of the sequential publisher, per level: an
# already published version is skipped, a rate limit is retried, and the
# first crate (in canonical order) that fails stops the publication with its
# exit status (124 after MEERKAT_CRATE_PUBLISH_TIMEOUT) once the crates of
# its level already in flight finish; no later level starts.

set -euo pipefail

ROOT="${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
DRY_RUN="${MEERKAT_REGISTRY_DRY_RUN:-false}"
PUBLISH_TIMEOUT="${MEERKAT_CRATE_PUBLISH_TIMEOUT:-20m}"
NO_VERIFY="${MEERKAT_CARGO_PUBLISH_NO_VERIFY:-true}"
RUNNER_TEMP="${RUNNER_TEMP:-/tmp}"
INDEX_URL="${MEERKAT_CRATE_INDEX_URL:-https://index.crates.io}"
INDEX_TIMEOUT="${MEERKAT_CRATE_INDEX_TIMEOUT:-600}"
INDEX_POLL="${MEERKAT_CRATE_INDEX_POLL_SECONDS:-5}"

case "$DRY_RUN" in
  1|true|TRUE|yes|YES|on|ON) DRY_RUN=true ;;
  *) DRY_RUN=false ;;
esac

case "$NO_VERIFY" in
  1|true|TRUE|yes|YES|on|ON) NO_VERIFY=true ;;
  *) NO_VERIFY=false ;;
esac

if [[ "$DRY_RUN" == "false" && "$NO_VERIFY" == "true" ]]; then
  JOBS="${MEERKAT_CRATE_PUBLISH_JOBS:-6}"
else
  JOBS="${MEERKAT_CRATE_PUBLISH_JOBS:-1}"
fi
if ! [[ "$JOBS" =~ ^[0-9]+$ ]] || ((JOBS < 1)); then
  echo "MEERKAT_CRATE_PUBLISH_JOBS must be a positive integer, got ${JOBS}" >&2
  exit 2
fi
if ! [[ "$INDEX_TIMEOUT" =~ ^[0-9]+$ && "$INDEX_POLL" =~ ^[0-9]+$ ]]; then
  echo "MEERKAT_CRATE_INDEX_TIMEOUT and MEERKAT_CRATE_INDEX_POLL_SECONDS must be integers" >&2
  exit 2
fi

publish_one() {
  local crate="$1"
  local tmp_cfg
  local output_file
  local target_dir
  local rc

  tmp_cfg="$(mktemp)"
  output_file="$(mktemp)"
  target_dir="${RUNNER_TEMP}/meerkat-publish-target/${crate}"

  "$ROOT/scripts/generate-patch-config.sh" "$ROOT" "$crate" > "$tmp_cfg"
  mkdir -p "${target_dir}/package"
  ln -sfn "${ROOT}/meerkat-core" "${target_dir}/package/meerkat-core"

  if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
    echo "::group::Publish Rust crate ${crate}"
  fi

  echo "  ${crate}: starting"
  if [[ "$DRY_RUN" == "true" ]]; then
    echo "  ${crate}: cargo publish dry-run"
    set +e
    CARGO_TARGET_DIR="$target_dir" \
      cargo publish -p "$crate" --locked --dry-run --config "$tmp_cfg" \
      > >(tee "$output_file") 2> >(tee -a "$output_file" >&2)
    rc=$?
    set -e
  else
    local cmd=(cargo publish -p "$crate" --locked --config "$tmp_cfg")
    if [[ "$NO_VERIFY" == "true" ]]; then
      cmd+=(--no-verify)
      echo "  ${crate}: upload mode skips cargo's duplicate verifier; release validation already packaged and linked these crates"
    fi

    # crates.io allows a burst of new versions of existing crates and then
    # refills about one per minute; a 43-crate release exhausts the burst.
    # The 429 body names the moment the next publish is accepted, so wait for
    # exactly that instead of retrying blind.
    local attempt=1
    local max_attempts="${MEERKAT_CRATE_PUBLISH_ATTEMPTS:-12}"
    if ! [[ "$max_attempts" =~ ^[0-9]+$ ]] || ((max_attempts < 1)); then
      max_attempts=12
    fi

    set +e
    while true; do
      : > "$output_file"
      if command -v timeout >/dev/null 2>&1; then
        CARGO_TARGET_DIR="$target_dir" \
          timeout "$PUBLISH_TIMEOUT" "${cmd[@]}" \
          > >(tee "$output_file") 2> >(tee -a "$output_file" >&2)
        rc=$?
      else
        CARGO_TARGET_DIR="$target_dir" \
          "${cmd[@]}" \
          > >(tee "$output_file") 2> >(tee -a "$output_file" >&2)
        rc=$?
      fi

      if [[ $rc -eq 0 ]] || ! grep -q "Too Many Requests" "$output_file" || ((attempt >= max_attempts)); then
        break
      fi

      local wait_seconds=60
      local retry_after
      retry_after="$(sed -n 's/.*try again after \([^,]*, [0-9]* [A-Za-z]* [0-9]* [0-9:]* GMT\).*/\1/p' "$output_file" | head -n1)"
      if [[ -n "$retry_after" ]]; then
        local retry_epoch now_epoch
        retry_epoch="$(date -u -d "$retry_after" +%s 2>/dev/null || echo "")"
        now_epoch="$(date -u +%s)"
        if [[ -n "$retry_epoch" ]] && ((retry_epoch > now_epoch)); then
          wait_seconds=$((retry_epoch - now_epoch + 2))
        fi
      fi
      echo "  ${crate}: crates.io rate limited publish attempt ${attempt}/${max_attempts}; retrying in ${wait_seconds}s" >&2
      sleep "$wait_seconds"
      attempt=$((attempt + 1))
    done
    set -e
  fi

  if [[ $rc -eq 0 ]]; then
    echo "  ${crate}: published"
  elif grep -Eq "already exists|already uploaded" "$output_file"; then
    echo "  ${crate}: already published, skipping"
  elif [[ $rc -eq 124 ]]; then
    echo "  ${crate}: FAILED after ${PUBLISH_TIMEOUT}" >&2
    rm -f "$tmp_cfg" "$output_file"
    if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
      echo "::endgroup::"
    fi
    exit 124
  else
    echo "  ${crate}: FAILED" >&2
    rm -f "$tmp_cfg" "$output_file"
    if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
      echo "::endgroup::"
    fi
    exit "$rc"
  fi

  rm -f "$tmp_cfg" "$output_file"
  if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
    echo "::endgroup::"
  fi
}

# Runs publish_one in its own errexit subshell, logs to <dir>/<crate>.log and
# records its exit status in <dir>/<crate>.rc.
publish_worker() {
  local crate="$1"
  local dir="$2"
  local rc
  set +e
  (
    set -e
    publish_one "$crate"
  ) >"${dir}/${crate}.log" 2>&1
  rc=$?
  set -e
  printf '%s\n' "$rc" >"${dir}/${crate}.rc"
  echo "  ${crate}: finished (exit ${rc})"
}

# crates.io sparse index path of a crate name.
index_path() {
  local name="${1,,}"
  case "${#name}" in
    1) printf '1/%s' "$name" ;;
    2) printf '2/%s' "$name" ;;
    3) printf '3/%s/%s' "${name:0:1}" "$name" ;;
    *) printf '%s/%s/%s' "${name:0:2}" "${name:2:2}" "$name" ;;
  esac
}

crate_visible() {
  local crate="$1"
  local version="$2"
  curl -fsSL --max-time 30 -H 'Cache-Control: no-cache' "${INDEX_URL}/$(index_path "$crate")" 2>/dev/null |
    jq -es --arg version "$version" 'any(.[]; .vers == $version)' >/dev/null 2>&1
}

# wait_for_index LEVEL CRATE:VERSION...: every crate of the level in the index.
wait_for_index() {
  local level="$1"
  shift
  local pending=("$@")
  local deadline=$(( $(date +%s) + INDEX_TIMEOUT ))
  local still entry
  while true; do
    still=()
    for entry in "${pending[@]}"; do
      crate_visible "${entry%%:*}" "${entry#*:}" || still+=("$entry")
    done
    if [[ "${#still[@]}" -eq 0 ]]; then
      echo "Level ${level}: every crate is visible in ${INDEX_URL}"
      return 0
    fi
    if (( $(date +%s) >= deadline )); then
      echo "Level ${level}: not visible in ${INDEX_URL} after ${INDEX_TIMEOUT}s: ${still[*]}" >&2
      return 1
    fi
    pending=("${still[@]}")
    sleep "$INDEX_POLL"
  done
}

# publish_level LEVEL CRATE:VERSION...
publish_level() {
  local level="$1"
  shift
  local entries=("$@")
  local dir="${WORK_DIR}/level-${level}"
  local entry crate rc failed_crate="" failed_rc=0
  mkdir -p "$dir"
  local names=()
  for entry in "${entries[@]}"; do
    names+=("${entry%%:*}")
  done
  echo "Level ${level}: publishing ${names[*]} (at most ${JOBS} at a time)"
  for crate in "${names[@]}"; do
    while [[ "$(jobs -rp | wc -l)" -ge "$JOBS" ]]; do
      wait -n || true
    done
    publish_worker "$crate" "$dir" &
  done
  wait || true
  for crate in "${names[@]}"; do
    cat "${dir}/${crate}.log"
    rc="$(cat "${dir}/${crate}.rc" 2>/dev/null || echo 1)"
    if [[ "$rc" -ne 0 && -z "$failed_crate" ]]; then
      failed_crate="$crate"
      failed_rc="$rc"
    fi
  done
  if [[ -n "$failed_crate" ]]; then
    echo "Level ${level}: ${failed_crate} failed (exit ${failed_rc}); no later level is published" >&2
    exit "$failed_rc"
  fi
  if [[ "$DRY_RUN" == "true" ]]; then
    return 0
  fi
  wait_for_index "$level" "${entries[@]}"
}

if [[ "$DRY_RUN" == "true" ]]; then
  echo "Running cargo publish in dry-run mode, by dependency level"
else
  echo "Publishing crates to crates.io, by dependency level"
  for tool in curl jq; do
    command -v "$tool" >/dev/null 2>&1 || { echo "${tool} is required to wait for the crates.io index" >&2; exit 1; }
  done
fi

# Canonical order within each level; the levels in ascending order (a level's
# crates are not contiguous in the canonical order).
levels="$("$ROOT/scripts/release-rust-crates.sh" --levels | sort -s -t $'\t' -k1,1n)"
WORK_DIR="$(mktemp -d "${RUNNER_TEMP}/meerkat-publish-levels.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT

current_level=""
level_entries=()
while IFS=$'\t' read -r level crate version; do
  [[ -n "$level" ]] || continue
  if [[ -n "$current_level" && "$level" != "$current_level" ]]; then
    publish_level "$current_level" "${level_entries[@]}"
    level_entries=()
  fi
  current_level="$level"
  level_entries+=("${crate}:${version}")
done <<<"$levels"
if [[ "${#level_entries[@]}" -gt 0 ]]; then
  publish_level "$current_level" "${level_entries[@]}"
fi
echo "Every release crate is published"
