#!/usr/bin/env bash
# Contract test for the "Install sccache" step of
# .github/actions/setup-sccache-ci, run as written in action.yml (read through
# the release contract's own action reader) against a stub gh, a stub curl and
# stand-in release archives:
# - the release comes from `gh release download` with the job's token first,
#   and from the anonymous release URL only when that fails or there is no
#   token; the token is unset before the installed binary runs;
# - a download that fails both ways, as the release host's 403 to every
#   anonymous try did, leaves the lane without sccache instead of failing it:
#   RUSTC_WRAPPER empty (no wrapper for Cargo), a warning that names both
#   causes, installed=false;
# - an archive whose sha256 does not match the pin, tampered or truncated and
#   from either path, still fails the lane before the binary reaches PATH or
#   runs;
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
(out / "install-token").write_text(install.env.get("GH_TOKEN", "") + "\n", encoding="utf-8")
with (out / "later-steps.tsv").open("w", encoding="utf-8") as handle:
    for step in steps[at + 1 :]:
        handle.write(f"{step.name}\t{step.condition or ''}\n")
PY

version="$(cat "$TEST_ROOT/version")"
name="sccache-${version}-x86_64-unknown-linux-musl"
release_url="https://github.com/mozilla/sccache/releases/download/${version}/${name}.tar.gz"
gh_403="HTTP 403: Forbidden (https://api.github.com/repos/mozilla/sccache/releases/tags/${version})"
curl_403="The requested URL returned error: 403"

# Stand-ins for gh and curl: each records how it was called, then serves
# STUB_<TOOL>_SERVE to its output path or fails the way the release host
# answered (gh exits 1, curl -f exits 22 after one 403 per try).
mkdir -p "$TEST_ROOT/stub-bin"
cat > "$TEST_ROOT/stub-bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s token=%s\n' "$*" "${GH_TOKEN:+set}" >> "$STUB_GH_LOG"
out=""
args=("$@")
for ((i = 0; i < ${#args[@]}; i++)); do
  if [[ "${args[i]}" == "-O" ]]; then out="${args[i + 1]}"; fi
done
case "$STUB_GH_MODE" in
  403)
    echo "HTTP 403: Forbidden (https://api.github.com/repos/mozilla/sccache/releases/tags/${args[2]})" >&2
    exit 1
    ;;
  serve)
    cp "$STUB_GH_SERVE" "$out"
    ;;
esac
EOF
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
chmod +x "$TEST_ROOT/stub-bin/gh" "$TEST_ROOT/stub-bin/curl"

# Stand-in release archives laid out like the real one; each binary records
# that it ran, and whether it saw a GH_TOKEN.
make_archive() {
  local label="$1" src="$TEST_ROOT/src-$1/$name"
  mkdir -p "$src"
  printf '#!/usr/bin/env bash\necho "%s $* token=${GH_TOKEN:+set}" >> "$SCCACHE_TEST_RAN"\necho "sccache 0.0.0-%s"\n' \
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
# runner's files, the release's sha256 pinned. <token> is "token" when the
# job token is in the step's env.
run_step() {
  local label="$1" token="$2" gh_mode="$3" gh_serve="$4" curl_mode="$5" curl_serve="$6"
  case_dir="$TEST_ROOT/case-$label"
  mkdir -p "$case_dir/runner-temp"
  : > "$case_dir/github-env"
  : > "$case_dir/github-path"
  : > "$case_dir/github-output"
  local token_env=()
  if [[ "$token" == token ]]; then token_env=(GH_TOKEN=job-token); fi
  local rc=0
  env -i PATH="$TEST_ROOT/stub-bin:$PATH" "${token_env[@]}" \
    RUNNER_OS=Linux RUNNER_ARCH=X64 RUNNER_TEMP="$case_dir/runner-temp" \
    GITHUB_ENV="$case_dir/github-env" GITHUB_PATH="$case_dir/github-path" \
    GITHUB_OUTPUT="$case_dir/github-output" \
    SCCACHE_VERSION="$version" SCCACHE_SHA256="$pinned" \
    STUB_GH_MODE="$gh_mode" STUB_GH_SERVE="$gh_serve" STUB_GH_LOG="$case_dir/gh.log" \
    STUB_CURL_MODE="$curl_mode" STUB_CURL_SERVE="$curl_serve" STUB_CURL_LOG="$case_dir/curl.log" \
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
ran_once_without_token() { [[ "$(cat "$case_dir/ran" 2> /dev/null)" == "release --version token=" ]]; }
no_binary() { [[ ! -e "$case_dir/runner-temp/sccache-install/bin/sccache" ]]; }
no_archive_left() { [[ ! -e "$case_dir/runner-temp/sccache-install/${name}.tar.gz" ]]; }
no_warning() { ! grep -q '^::warning::' "$case_dir/stdout"; }
sha_check_failed() { grep -q ': FAILED$' "$case_dir/stdout"; }
asked_gh_for_release() {
  [[ "$(cat "$case_dir/gh.log")" == "release download ${version} -R mozilla/sccache -p ${name}.tar.gz -O $case_dir/runner-temp/sccache-install/${name}.tar.gz --clobber token=set" ]]
}
gh_not_called() { [[ ! -e "$case_dir/gh.log" ]]; }
asked_curl_for_release() { [[ "$(cat "$case_dir/curl.log")" == "$release_url" ]]; }
curl_not_called() { [[ ! -e "$case_dir/curl.log" ]]; }
warns() {
  has_line stdout "::warning::sccache ${version} download failed ($1); this job compiles without sccache"
}
warns_both_403s() { warns "gh exit 1: ${gh_403}; curl exit 22: ${curl_403}"; }
warns_no_token_403() { warns "gh no token; curl exit 22: ${curl_403}"; }
installed() {
  check "$1: it installs" exit_is 0
  check "$1: it reports installed=true" has_line github-output "installed=true"
  check "$1: its bin directory reaches PATH" has_line github-path "$case_dir/runner-temp/sccache-install/bin"
  check "$1: SCCACHE_PATH names the installed binary" \
    has_line github-env "SCCACHE_PATH=$case_dir/runner-temp/sccache-install/bin/sccache"
  check "$1: RUSTC_WRAPPER is left to the job" lacks_prefix github-env "RUSTC_WRAPPER="
  check "$1: the verified binary ran once, for its version, without the token" ran_once_without_token
  check "$1: no warning" no_warning
}
not_installed() {
  check "$1: the lane does not fail" exit_is 0
  check "$1: it reports installed=false" has_line github-output "installed=false"
  check "$1: it empties RUSTC_WRAPPER for the later steps" has_line github-env "RUSTC_WRAPPER="
  check "$1: nothing reaches PATH" is_empty github-path
  check "$1: no SCCACHE_PATH is exported" lacks_prefix github-env "SCCACHE_PATH="
  check "$1: no partial archive is left" no_archive_left
  check "$1: no sccache binary ran" never_ran
}
fails_closed() {
  check "$1: the lane fails" exit_is_not 0
  check "$1: the sha256 check is what failed" sha_check_failed
  check "$1: nothing reaches PATH" is_empty github-path
  check "$1: no SCCACHE_PATH is exported" lacks_prefix github-env "SCCACHE_PATH="
  check "$1: RUSTC_WRAPPER is left to the job" lacks_prefix github-env "RUSTC_WRAPPER="
  check "$1: no installed output" is_empty github-output
  check "$1: nothing was extracted" no_binary
  check "$1: no sccache binary ran" never_ran
}

run_step gh token serve "$TEST_ROOT/release.tar.gz" 403 ""
check "gh: the release comes from gh release download with the job token" asked_gh_for_release
check "gh: the anonymous URL is not tried" curl_not_called
installed gh

run_step gh-403 token 403 "" serve "$TEST_ROOT/release.tar.gz"
check "gh-403: gh was tried first" asked_gh_for_release
check "gh-403: then the anonymous release URL" asked_curl_for_release
installed gh-403

run_step no-token none 403 "" serve "$TEST_ROOT/release.tar.gz"
check "no-token: gh is not tried without a token" gh_not_called
check "no-token: the anonymous release URL is" asked_curl_for_release
installed no-token

run_step both-403 token 403 "" 403 ""
check "both-403: gh was tried first" asked_gh_for_release
check "both-403: then the anonymous release URL" asked_curl_for_release
check "both-403: the warning names both causes" warns_both_403s
not_installed both-403

run_step no-token-403 none 403 "" 403 ""
check "no-token-403: the warning says there was no token" warns_no_token_403
not_installed no-token-403

run_step gh-tampered token serve "$TEST_ROOT/tampered.tar.gz" 403 ""
check "gh-tampered: the anonymous URL is not tried" curl_not_called
fails_closed gh-tampered

run_step curl-truncated none 403 "" serve "$TEST_ROOT/truncated.tar.gz"
fails_closed curl-truncated

# The token is the job's own, and every later step (authentication, backend
# selection, server start) runs only once sccache is installed.
case_dir="$TEST_ROOT/structure"
mkdir -p "$case_dir"
echo "-" > "$case_dir/rc"
: > "$case_dir/stdout"
: > "$case_dir/stderr"
install_step_has_id() { [[ "$(cat "$TEST_ROOT/install-id")" == "install" ]]; }
check "the install step has id install" install_step_has_id
token_is_the_jobs() { [[ "$(cat "$TEST_ROOT/install-token")" == '${{ github.token }}' ]]; }
check "the install step's GH_TOKEN is the job's own token" token_is_the_jobs
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
