#!/usr/bin/env bash
# Contract test for scripts/ci-pr-classification-base: a pull_request run is
# classified against the first parent of the checked-out merge commit, never
# the (possibly stale) event payload base.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
resolver="${ROOT}/scripts/ci-pr-classification-base"
repo="$(mktemp -d "${TMPDIR:-/tmp}/ci-pr-base.XXXXXX")"
trap 'rm -rf "$repo"' EXIT

g() { git -C "$repo" -c user.name=Meerkat -c user.email=meerkat@example.invalid "$@"; }
g init -q -b main
g commit --allow-empty -qm base
payload_base="$(g rev-parse HEAD)"
g checkout -qb pr
echo pr > "$repo/pr-only.txt"; g add pr-only.txt; g commit -qm pr
pr_head="$(g rev-parse HEAD)"
g checkout -q main
echo main > "$repo/main-only.txt"; g add main-only.txt; g commit -qm "main advanced after the event was queued"
advanced_main="$(g rev-parse HEAD)"
# GitHub's refs/pull/N/merge: base-branch tip first, PR head second.
g merge -q --no-ff -m merge "$pr_head"

resolved="$(cd "$repo" && "$resolver" pull_request "$payload_base" 2>/dev/null)"
if [[ "$resolved" != "$advanced_main" ]]; then
  echo "pull_request base resolved to ${resolved}, expected the merge commit's first parent ${advanced_main}" >&2
  exit 1
fi
changed="$(g diff --name-only "${resolved}...HEAD")"
if [[ "$changed" != "pr-only.txt" ]]; then
  echo "classification diff includes main's own changes: ${changed}" >&2
  exit 1
fi
stale_changed="$(g diff --name-only "${payload_base}...HEAD" | sort | paste -sd, -)"
if [[ "$stale_changed" != "main-only.txt,pr-only.txt" ]]; then
  echo "fixture no longer reproduces the stale payload-base race: ${stale_changed}" >&2
  exit 1
fi

# push events keep the payload base.
if [[ "$(cd "$repo" && "$resolver" push "$payload_base")" != "$payload_base" ]]; then
  echo "push base must be the payload base" >&2
  exit 1
fi

# The changes job checks out the merge commit at depth 2. That shallow clone
# still resolves the base parent, holds it, and yields the same changed
# paths through the two-dot diff the classifier runs; a depth-1 clone hides
# the parents, so the resolver fails closed there.
shallow2="$(mktemp -d "${TMPDIR:-/tmp}/ci-pr-base-depth2.XXXXXX")"
shallow1="$(mktemp -d "${TMPDIR:-/tmp}/ci-pr-base-depth1.XXXXXX")"
trap 'rm -rf "$repo" "$shallow2" "$shallow1"' EXIT
git clone -q --depth 2 "file://${repo}" "$shallow2/clone"
git clone -q --depth 1 "file://${repo}" "$shallow1/clone"
if [[ "$(git -C "$shallow2/clone" rev-parse HEAD)" != "$(g rev-parse HEAD)" ]]; then
  echo "depth-2 clone did not check out the merge commit" >&2
  exit 1
fi
shallow_resolved="$(cd "$shallow2/clone" && "$resolver" pull_request "$payload_base" 2>/dev/null)" || {
  echo "depth-2 checkout could not resolve the merge commit's base parent" >&2
  exit 1
}
if [[ "$shallow_resolved" != "$advanced_main" ]]; then
  echo "depth-2 checkout resolved ${shallow_resolved}, expected ${advanced_main}" >&2
  exit 1
fi
if ! git -C "$shallow2/clone" cat-file -e "${shallow_resolved}^{commit}"; then
  echo "depth-2 checkout lacks the resolved base commit" >&2
  exit 1
fi
shallow_changed="$(git -C "$shallow2/clone" diff --name-only "$shallow_resolved" HEAD)"
if [[ "$shallow_changed" != "pr-only.txt" ]]; then
  echo "depth-2 classification diff is ${shallow_changed}, expected pr-only.txt" >&2
  exit 1
fi
if (cd "$shallow1/clone" && "$resolver" pull_request "$payload_base") >/dev/null 2>&1; then
  echo "a depth-1 checkout must fail closed: it hides the merge parents" >&2
  exit 1
fi

# A pull_request checkout that is not a merge commit fails closed.
g checkout -q "$pr_head"
if (cd "$repo" && "$resolver" pull_request "$payload_base") >/dev/null 2>&1; then
  echo "a non-merge pull_request checkout must fail closed" >&2
  exit 1
fi

echo "ci-pr-classification-base contracts hold"
