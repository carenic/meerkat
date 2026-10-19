#!/usr/bin/env bash
# Contract test for the "Install sccache" step of
# .github/actions/setup-sccache-ci, run as written in action.yml (read through
# the release contract's own action reader) against a stub curl and stand-in
# release archives:
# - a download that fails, as the release host's 403 to every try did, leaves
#   the lane without sccache instead of failing it: RUSTC_WRAPPER empty (no
#   wrapper for Cargo), a warning that names the cause, installed=false;
# - a download whose sha256 does not match the pin, tampered or truncated,
#   still fails the lane before the binary reaches PATH or runs;
# - the pinned release installs as before;
# - every later step of the action runs only once sccache is installed.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PYTHON="${PYTHON:-python3}"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/setup-sccache-ci.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

"$PYTHON" - "$REPO_ROOT" "$TEST_ROOT" <<'PY'
import sys
from pathlib import Path

root, out = Path(sys.argv[1]), Path(sys.argv[2])
sys.path.insert(0, str(root / "scripts"))
import check_release_workflow_contract as contract

steps = list(contract._local_action(contract.SCCACHE_ACTION, root).steps)
names = [step.name for step in steps]
if "Install sccache" not in names:
    sys.exit(f"{contract.SCCACHE_ACTION} has no `Install sccache` step (steps: {names})")
at = names.index("Install sccache")
install = steps[at]
(out / "install.sh").write_text(install.raw_run, encoding="utf-8")
(out / "version").write_text(install.env.get("SCCACHE_VERSION", "") + "\n", encoding="utf-8")
(out / "install-id").write_text(install.fields.get("id", "") + "\n", encoding="utf-8")
with (out / "later-steps.tsv").open("w", encoding="utf-8") as handle:
    for step in steps[at + 1 :]:
        handle.write(f"{step.name}\t{step.condition or ''}\n")
PY

version="$(cat "$TEST_ROOT/version")"
name="sccache-${version}-x86_64-unknown-linux-musl"
release_url="https://github.com/mozilla/sccache/releases/download/${version}/${name}.tar.gz"

# A stand-in curl: records the URL it was asked for, then either serves
# STUB_CURL_SERVE to its -o path or fails the way the release host did, one
# 403 per try (curl -f exits 22).
mkdir -p "$TEST_ROOT/stub-bin"
cat > "$TEST_ROOT/stub-bin/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
out=""
args=("$@")
for ((i = 0; i < ${#args[@]}; i++)); do
  if [[ "${args[i]}" == "-o" ]]; then out="${args[i + 1]}"; fi
done
printf '%s\n' "${args[${#args[@]} - 1]}" >> "$STUB_CURL_LOG"
case "$STUB_CURL_MODE" in
  403)
    for _ in 1 2 3 4 5 6 7 8 9; do
      echo "curl: (22) The requested URL returned error: 403" >&2
    done
    exit 22
    ;;
  serve)
    cp "$STUB_CURL_SERVE" "$out"
    ;;
esac
EOF
chmod +x "$TEST_ROOT/stub-bin/curl"

# Stand-in release archives laid out like the real one; each binary records
# that it ran.
make_archive() {
  local label="$1" src="$TEST_ROOT/src-$1/$name"
  mkdir -p "$src"
  printf '#!/usr/bin/env bash\necho "%s $*" >> "$SCCACHE_TEST_RAN"\necho "sccache 0.0.0-%s"\n' \
    "$label" "$label" > "$src/sccache"
  chmod +x "$src/sccache"
  tar -czf "$TEST_ROOT/$label.tar.gz" -C "$TEST_ROOT/src-$label" "$name"
}
make_archive release
make_archive tampered
head -c 64 "$TEST_ROOT/release.tar.gz" > "$TEST_ROOT/truncated.tar.gz"
pinned="$(sha256sum "$TEST_ROOT/release.tar.gz" | cut -d' ' -f1)"

failures=0
case_dir=""

# Runs the step the way a composite bash step runs: the step's env plus the
# runner's files, the release's sha256 pinned.
run_step() {
  local label="$1" mode="$2" serve="${3:-}"
  case_dir="$TEST_ROOT/case-$label"
  mkdir -p "$case_dir/runner-temp"
  : > "$case_dir/github-env"
  : > "$case_dir/github-path"
  : > "$case_dir/github-output"
  local rc=0
  env -i PATH="$TEST_ROOT/stub-bin:$PATH" \
    RUNNER_OS=Linux RUNNER_ARCH=X64 RUNNER_TEMP="$case_dir/runner-temp" \
    GITHUB_ENV="$case_dir/github-env" GITHUB_PATH="$case_dir/github-path" \
    GITHUB_OUTPUT="$case_dir/github-output" \
    SCCACHE_VERSION="$version" SCCACHE_SHA256="$pinned" \
    STUB_CURL_MODE="$mode" STUB_CURL_SERVE="$serve" STUB_CURL_LOG="$case_dir/curl.log" \
    SCCACHE_TEST_RAN="$case_dir/ran" \
    bash --noprofile --norc -eo pipefail "$TEST_ROOT/install.sh" \
    > "$case_dir/stdout" 2> "$case_dir/stderr" || rc=$?
  echo "$rc" > "$case_dir/rc"
}

check() {
  local label="$1"
  shift
  if "$@"; then
    printf 'PASS %s\n' "$label"
  else
    printf 'FAIL %s\n' "$label" >&2
    failures=$((failures + 1))
    printf '  step exit %s; stdout tail:\n' "$(cat "$case_dir/rc")" >&2
    tail -n 5 "$case_dir/stdout" | sed 's/^/    /' >&2
    printf '  stderr tail:\n' >&2
    tail -n 5 "$case_dir/stderr" | sed 's/^/    /' >&2
  fi
}

exit_is() { [[ "$(cat "$case_dir/rc")" == "$1" ]]; }
exit_is_not() { [[ "$(cat "$case_dir/rc")" != "$1" ]]; }
has_line() { grep -Fxq -- "$2" "$case_dir/$1"; }
lacks_prefix() { ! grep -q -- "^$2" "$case_dir/$1"; }
is_empty() { [[ ! -s "$case_dir/$1" ]]; }
never_ran() { [[ ! -e "$case_dir/ran" ]]; }
ran_once_for_version() { [[ "$(cat "$case_dir/ran" 2> /dev/null)" == "release --version" ]]; }
no_binary() { [[ ! -e "$case_dir/runner-temp/sccache-install/bin/sccache" ]]; }
no_archive_left() { [[ ! -e "$case_dir/runner-temp/sccache-install/${name}.tar.gz" ]]; }
warns_403() {
  has_line stdout "::warning::sccache ${version} download failed (curl exit 22: The requested URL returned error: 403); this job compiles without sccache"
}
no_warning() { ! grep -q '^::warning::' "$case_dir/stdout"; }
sha_check_failed() { grep -q ': FAILED$' "$case_dir/stdout"; }
asked_for_release() { [[ "$(cat "$case_dir/curl.log")" == "$release_url" ]]; }

run_step 403 403
check "a 403 on every try does not fail the lane" exit_is 0
check "it asked for the pinned release" asked_for_release
check "it reports installed=false" has_line github-output "installed=false"
check "it empties RUSTC_WRAPPER for the later steps" has_line github-env "RUSTC_WRAPPER="
check "it warns with the cause" warns_403
check "nothing reaches PATH" is_empty github-path
check "no SCCACHE_PATH is exported" lacks_prefix github-env "SCCACHE_PATH="
check "no partial archive is left" no_archive_left
check "no sccache binary ran" never_ran

run_step release serve "$TEST_ROOT/release.tar.gz"
check "the pinned release installs" exit_is 0
check "it reports installed=true" has_line github-output "installed=true"
check "its bin directory reaches PATH" has_line github-path "$case_dir/runner-temp/sccache-install/bin"
check "SCCACHE_PATH names the installed binary" \
  has_line github-env "SCCACHE_PATH=$case_dir/runner-temp/sccache-install/bin/sccache"
check "RUSTC_WRAPPER is left to the job" lacks_prefix github-env "RUSTC_WRAPPER="
check "the verified binary ran once, for its version" ran_once_for_version
check "no warning" no_warning

for label in tampered truncated; do
  run_step "$label" serve "$TEST_ROOT/$label.tar.gz"
  check "a $label download fails the lane" exit_is_not 0
  check "$label: the sha256 check is what failed" sha_check_failed
  check "$label: nothing reaches PATH" is_empty github-path
  check "$label: no SCCACHE_PATH is exported" lacks_prefix github-env "SCCACHE_PATH="
  check "$label: RUSTC_WRAPPER is left to the job" lacks_prefix github-env "RUSTC_WRAPPER="
  check "$label: no installed output" is_empty github-output
  check "$label: nothing was extracted" no_binary
  check "$label: no sccache binary ran" never_ran
done

# Every later step (authentication, backend selection, server start) runs only
# once sccache is installed.
case_dir="$TEST_ROOT/structure"
mkdir -p "$case_dir"
echo "-" > "$case_dir/rc"
: > "$case_dir/stdout"
: > "$case_dir/stderr"
install_step_has_id() { [[ "$(cat "$TEST_ROOT/install-id")" == "install" ]]; }
check "the install step has id install" install_step_has_id
later_steps=0
while IFS=$'\t' read -r step condition; do
  later_steps=$((later_steps + 1))
  gated() { [[ "$condition" == *"steps.install.outputs.installed == 'true'"* ]]; }
  check "\`$step\` runs only once sccache is installed" gated
done < "$TEST_ROOT/later-steps.tsv"
has_later_steps() { (( later_steps >= 3 )); }
check "the action has its three later steps" has_later_steps

if (( failures > 0 )); then
  printf '%d setup-sccache-ci check(s) failed\n' "$failures" >&2
  exit 1
fi
printf 'setup-sccache-ci install step: all checks passed\n'
