#!/usr/bin/env bash
# Git's raw pre-push boundary. Unlike pre-commit's generic pre-push adapter,
# this sees every ref update on stdin. Validate one exact pushed object in an
# immutable detached worktree so concurrent edits cannot change tested bytes.
#
# Every nonzero exit from here must name its own cause. Git reports only
# "failed to push some refs", so an unattributed exit leaves the operator with
# no way to tell a failing hook from a failing dispatcher.
set -euo pipefail

dispatch_step="parsing hook arguments"

report_internal_failure() {
  local status="$1"
  local line="$2"
  printf '\n' >&2
  echo "Meerkat pre-push dispatcher failed while ${dispatch_step}" >&2
  echo "  exit status : ${status}" >&2
  echo "  location    : $(basename "${BASH_SOURCE[0]}"):${line}" >&2
  echo "This is the dispatcher itself, not a validation hook: no hook result" >&2
  echo "printed above is the cause of this push failure." >&2
}
trap 'report_internal_failure "$?" "$LINENO"' ERR

if [[ "$#" -ne 2 ]]; then
  echo "usage: pre-push-dispatch.sh <remote-name> <remote-url>" >&2
  exit 2
fi

REMOTE_NAME="$1"
REMOTE_URL="$2"
SOURCE_ROOT="$(git rev-parse --show-toplevel)"
ZERO_SHA="0000000000000000000000000000000000000000"
CACHE_VERSION="v1"

# Git exports repository-local variables to hooks. They must not cross into the
# detached validation worktree: nested git commands would otherwise continue
# targeting the source repository regardless of their cwd or `-C` argument.
while IFS= read -r git_local_env; do
  [[ -n "$git_local_env" ]] && unset "$git_local_env"
done < <(git -C "$SOURCE_ROOT" rev-parse --local-env-vars)

dispatch_step="reading pushed refs from stdin"
ref_count=0
local_ref=""
local_sha=""
remote_ref=""
remote_sha=""
while read -r next_local_ref next_local_sha next_remote_ref next_remote_sha; do
  [[ -n "${next_local_ref:-}" ]] || continue
  ref_count=$((ref_count + 1))
  local_ref="$next_local_ref"
  local_sha="$next_local_sha"
  remote_ref="$next_remote_ref"
  remote_sha="$next_remote_sha"
done

if [[ "$ref_count" -ne 1 ]]; then
  echo "Meerkat's pre-push gate requires exactly one ref update; received ${ref_count}." >&2
  echo "Push refs one at a time so every pushed object is validated exactly." >&2
  exit 1
fi

# A single deletion contains no local object to compile. It is safe to pass
# without running source validation; mixed deletion/update pushes are rejected
# by the single-ref rule above.
if [[ "$local_sha" == "$ZERO_SHA" ]]; then
  exit 0
fi

if ! pushed_commit="$(git -C "$SOURCE_ROOT" rev-parse --verify "${local_sha}^{commit}" 2>/dev/null)"; then
  echo "Pushed ref ${local_ref} does not resolve to a commit: ${local_sha}" >&2
  exit 1
fi
checked_out_commit="$(git -C "$SOURCE_ROOT" rev-parse --verify HEAD)"
if [[ "$pushed_commit" != "$checked_out_commit" ]]; then
  echo "Pre-push validation for ${local_ref} -> ${remote_ref} requires the pushed commit (${pushed_commit}) to equal checked-out HEAD (${checked_out_commit})." >&2
  echo "Push the checked-out branch or tag alone from its own checkout." >&2
  exit 1
fi

dispatch_step="resolving the exact-tree evidence cache"
pushed_tree="$(git -C "$SOURCE_ROOT" rev-parse "${pushed_commit}^{tree}")"
git_common_dir="$(git -C "$SOURCE_ROOT" rev-parse --git-common-dir)"
if [[ "$git_common_dir" != /* ]]; then
  git_common_dir="${SOURCE_ROOT}/${git_common_dir}"
fi
hook_cache_dir="${git_common_dir}/meerkat-hook-cache/exact-tree"
hook_stamp="${hook_cache_dir}/${CACHE_VERSION}-${pushed_tree}.ok"
mkdir -p "$hook_cache_dir"

if [[ "${MEERKAT_SKIP_PRE_PUSH_TREE_CACHE:-0}" != "1" && -f "$hook_stamp" ]]; then
  echo "complete pre-push gate already validated for tree ${pushed_tree}; reusing exact-tree evidence."
  exit 0
fi

dispatch_step="creating the detached validation worktree"
validation_root="$(mktemp -d "${TMPDIR:-/tmp}/meerkat-pre-push-exact.XXXXXX")"
validation_tree="${validation_root}/tree"
cleanup() {
  local pending_status=$?
  if [[ -d "$validation_tree" ]]; then
    if ! git -C "$SOURCE_ROOT" worktree remove --force "$validation_tree" >/dev/null 2>&1; then
      echo "note: validation worktree left behind: ${validation_tree}" >&2
      echo "      prune it with: git -C ${SOURCE_ROOT} worktree prune" >&2
    fi
  fi
  if ! rm -rf "$validation_root" 2>/dev/null; then
    echo "note: validation scratch directory left behind: ${validation_root}" >&2
  fi
  # Residue must never decide the push. A failing command inside an EXIT trap
  # under `set -e` otherwise rewrites a fully passing gate into a bare exit 1
  # with nothing but hook successes on screen, which is exactly the
  # unattributable push failure this dispatcher used to produce.
  exit "$pending_status"
}
trap cleanup EXIT

git -C "$SOURCE_ROOT" worktree add --detach --quiet "$validation_tree" "$pushed_commit"

export PRE_COMMIT_REMOTE_NAME="$REMOTE_NAME"
export PRE_COMMIT_REMOTE_URL="$REMOTE_URL"
export PRE_COMMIT_TO_REF="$pushed_commit"
if [[ "$remote_sha" == "$ZERO_SHA" ]]; then
  PRE_COMMIT_FROM_REF="$(git -C "$SOURCE_ROOT" hash-object -t tree /dev/null)"
else
  PRE_COMMIT_FROM_REF="$remote_sha"
fi
export PRE_COMMIT_FROM_REF
# repo-cargo already includes the common Git directory in its cache key. A
# stable lane keeps the detached validation worktree hot across pushes.
export RUST_LANE_ID="${RUST_LANE_ID:-pre-push}"

cd "$validation_tree"

# The hook transcript is captured so a failure can name the hook that caused
# it. Python buffers block-wise when its stdout is a pipe; unbuffering keeps
# the operator's terminal live through the long deterministic lanes.
gate_log="${validation_root}/push-stage-hooks.log"
dispatch_step="running push-stage validation hooks"
# Hook failure is a reported outcome, not a dispatcher fault: the ERR trap fires
# even with errexit disabled, so it is lifted for exactly this pipeline. Capture
# the whole PIPESTATUS array at once; any later command resets it.
trap - ERR
set +e
if [[ "$remote_sha" == "$ZERO_SHA" ]]; then
  PYTHONUNBUFFERED=1 pre-commit run --config .pre-commit-config.yaml \
    --hook-stage pre-push --all-files 2>&1 | tee "$gate_log"
else
  PYTHONUNBUFFERED=1 pre-commit run --config .pre-commit-config.yaml \
    --hook-stage pre-push --from-ref "$remote_sha" --to-ref "$pushed_commit" \
    2>&1 | tee "$gate_log"
fi
gate_pipeline_status=("${PIPESTATUS[@]}")
set -e
trap 'report_internal_failure "$?" "$LINENO"' ERR
gate_status="${gate_pipeline_status[0]}"
capture_status="${gate_pipeline_status[1]:-0}"

if [[ "$capture_status" -ne 0 ]]; then
  echo "note: could not capture the hook transcript (tee exit ${capture_status});" >&2
  echo "      failure attribution below may be incomplete." >&2
fi

if [[ "$gate_status" -ne 0 ]]; then
  gate_plain="$(sed -E $'s/\x1b\\[[0-9;]*[A-Za-z]//g' "$gate_log" 2>/dev/null || true)"
  failed_names="$(sed -n -E 's/^(.+[^.])[.]{2,}Failed$/\1/p' <<<"$gate_plain")"
  failed_ids="$(sed -n -E 's/^- hook id: (.+)$/\1/p' <<<"$gate_plain")"
  printf '\n' >&2
  echo "Meerkat pre-push gate FAILED (pre-commit exit ${gate_status})." >&2
  if [[ -n "$failed_names" ]]; then
    echo "Failing push-stage hook(s):" >&2
    while IFS= read -r failed_name; do
      if [[ -n "$failed_name" ]]; then
        echo "  - ${failed_name}" >&2
      fi
    done <<<"$failed_names"
    if [[ -n "$failed_ids" ]]; then
      echo "Rerun the failing hook alone with:" >&2
      while IFS= read -r failed_id; do
        if [[ -n "$failed_id" ]]; then
          echo "  pre-commit run --hook-stage pre-push --all-files ${failed_id}" >&2
        fi
      done <<<"$failed_ids"
    fi
  else
    echo "No hook reported Failed, so the failure is in the gate harness, not" >&2
    echo "in a validation hook. Last 40 transcript lines:" >&2
    tail -40 "$gate_log" >&2 || true
  fi
  exit "$gate_status"
fi

dispatch_step="recording exact-tree validation evidence"
stamp_tmp="${hook_stamp}.tmp.$$"
printf 'tree=%s\ncommit=%s\n' "$pushed_tree" "$pushed_commit" > "$stamp_tmp"
mv "$stamp_tmp" "$hook_stamp"
