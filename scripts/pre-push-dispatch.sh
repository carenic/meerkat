#!/usr/bin/env bash
# Git's raw pre-push boundary. Unlike pre-commit's generic pre-push adapter,
# this sees every ref update on stdin. Validate one exact pushed object in an
# immutable detached worktree so concurrent edits cannot change tested bytes.
set -euo pipefail

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

validation_root="$(mktemp -d "${TMPDIR:-/tmp}/meerkat-pre-push-exact.XXXXXX")"
validation_tree="${validation_root}/tree"
cleanup() {
  if [[ -d "$validation_tree" ]]; then
    git -C "$SOURCE_ROOT" worktree remove --force "$validation_tree" >/dev/null 2>&1 || true
  fi
  rm -rf "$validation_root"
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
if [[ "$remote_sha" == "$ZERO_SHA" ]]; then
  pre-commit run --config .pre-commit-config.yaml --hook-stage pre-push --all-files
else
  pre-commit run --config .pre-commit-config.yaml --hook-stage pre-push \
    --from-ref "$remote_sha" --to-ref "$pushed_commit"
fi

stamp_tmp="${hook_stamp}.tmp.$$"
printf 'tree=%s\ncommit=%s\n' "$pushed_tree" "$pushed_commit" > "$stamp_tmp"
mv "$stamp_tmp" "$hook_stamp"
