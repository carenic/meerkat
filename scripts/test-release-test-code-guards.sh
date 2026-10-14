#!/usr/bin/env bash
# Contract tests for the release test-code guards: the Bazel release feature
# gate (scripts/check-bazel-release-features.py) and the packaged-binary marker
# scan (scripts/check-release-binary-markers) must each still fail, naming the
# defect, on every way they exist to catch, and pass the clean repository.

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
PYTHON="$("${ROOT}/scripts/require-python" 3.11 "$(basename "${BASH_SOURCE[0]}")")" || exit 1

"${PYTHON}" "${ROOT}/scripts/test_check_bazel_release_features.py"
"${PYTHON}" "${ROOT}/scripts/test_check_release_binary_markers.py"
echo "Release test-code guard contract tests passed"
