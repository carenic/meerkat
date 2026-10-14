#!/usr/bin/env bash
# Contract tests for the release test-code guards: the Bazel release feature
# gate (scripts/check-bazel-release-features.py), the packaged-binary marker
# scan (scripts/check-release-binary-markers) and the release tree's test-hook
# gating check (scripts/check-release-source-test-hooks) must each still fail,
# naming the defect, on every way they exist to catch, and pass the clean
# repository.

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
PYTHON="$("${ROOT}/scripts/require-python" 3.11 "$(basename "${BASH_SOURCE[0]}")")" || exit 1

"${PYTHON}" "${ROOT}/scripts/test_check_bazel_release_features.py"
"${PYTHON}" "${ROOT}/scripts/test_check_release_binary_markers.py"
"${PYTHON}" "${ROOT}/scripts/test_check_release_source_test_hooks.py"
echo "Release test-code guard contract tests passed"
