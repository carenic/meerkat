#!/usr/bin/env bash
# Contract test for the self-hosted GCP BuildBuddy path: what the scripts
# would send to GCP and to BuildBuddy, checked against stubs. `gcloud` and
# `bb` are fakes on PATH that record their arguments; nothing here reaches
# GCP, BuildBuddy or any other endpoint.
#
#   - scripts/buildbuddy-bazel-poc never sends the BuildBuddy API key to a
#     non-TLS endpoint, however the endpoint is configured, and keeps the key
#     out of argv.
#   - scripts/gcp-buildbuddy-executor-pool sync-secrets writes only the
#     endpoint secret: the executor key secret is never written from CI, so
#     the BuildBuddy Cloud key cannot reach the executors.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/meerkat-gcp-bb-hardening.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

FAKE_BIN="${TEST_ROOT}/bin"
mkdir -p "${FAKE_BIN}"
FAKE_API_KEY="fake-buildbuddy-api-key-0123456789"
failures=0

fail() {
  echo "FAIL: $*" >&2
  failures=$((failures + 1))
}

pass() {
  echo "ok: $*"
}

# --- fake gcloud: answers from <state>/state.json, records every call (argv,
# stdin for --data-file=-, the contents of --metadata-from-file files) to
# <state>/calls.jsonl. Mutating verbs only record.
cat >"${FAKE_BIN}/gcloud" <<'PY'
#!/usr/bin/env python3
import json
import os
import re
import sys

STATE_DIR = os.environ["FAKE_GCLOUD_STATE"]
with open(os.path.join(STATE_DIR, "state.json"), encoding="utf-8") as handle:
    STATE = json.load(handle)
ARGV = sys.argv[1:]
BOOLEAN_FLAGS = {
    "quiet", "async", "all", "stable", "no-address", "tunnel-through-iap",
    "disabled", "no-disabled", "enable-logging", "iap-tunnel-disable-connection-check",
}

flags = {}
positionals = []
index = 0
while index < len(ARGV):
    token = ARGV[index]
    if token.startswith("--"):
        name, has_value, value = token[2:].partition("=")
        if not has_value and name not in BOOLEAN_FLAGS and index + 1 < len(ARGV) and not ARGV[index + 1].startswith("--"):
            value = ARGV[index + 1]
            index += 1
        flags.setdefault(name, []).append(value if (has_value or name not in BOOLEAN_FLAGS) else True)
    else:
        positionals.append(token)
    index += 1

record = {"argv": ARGV}
for spec in flags.get("metadata-from-file", []):
    for item in str(spec).split(","):
        key, _, path = item.partition("=")
        try:
            with open(path, encoding="utf-8") as handle:
                record.setdefault("files", {})[key] = handle.read()
        except OSError:
            record.setdefault("files", {})[key] = None
if flags.get("data-file") == ["-"]:
    record["stdin"] = sys.stdin.read()
with open(os.path.join(STATE_DIR, "calls.jsonl"), "a", encoding="utf-8") as handle:
    handle.write(json.dumps(record) + "\n")


def flag(name):
    values = flags.get(name)
    return values[-1] if values else None


def get_path(obj, path):
    for part in re.findall(r"[A-Za-z_][A-Za-z0-9_]*|\[\d+\]", path):
        if part.startswith("["):
            idx = int(part[1:-1])
            obj = obj[idx] if isinstance(obj, list) and idx < len(obj) else None
        else:
            obj = obj.get(part) if isinstance(obj, dict) else None
        if obj is None:
            return ""
    return obj


def emit(obj):
    fmt = flag("format")
    if fmt is None or fmt == "json":
        print(json.dumps(obj))
        return
    match = re.match(r"value\((.*)\)$", fmt)
    if match:
        rows = obj if isinstance(obj, list) else [obj]
        for row in rows:
            print("\t".join(str(get_path(row, field.strip())) for field in match.group(1).split(",")))
        return
    print("(fake gcloud table)")


def missing(what):
    print(f"ERROR: (gcloud) {what} was not found", file=sys.stderr)
    sys.exit(1)


words = positionals
name = words[3] if len(words) > 3 else None
if words[:2] == ["compute", "firewall-rules"]:
    rules = STATE.get("firewall_rules", [])
    if words[2] == "list":
        emit(rules)
    elif words[2] == "describe":
        found = [rule for rule in rules if rule.get("name") == name]
        emit(found[0]) if found else missing(f"firewall rule {name}")
elif words[:2] == ["compute", "instances"]:
    instance = STATE.get("instances", {}).get(name)
    if words[2] == "describe":
        emit(instance) if instance is not None else missing(f"instance {name}")
    elif words[2] == "get-guest-attributes":
        if instance is None:
            missing(f"instance {name}")
        prefix = (flag("query-path") or "").rstrip("/")
        attributes = [
            item for item in STATE.get("guest_attributes", {}).get(name, [])
            if not prefix or item.get("namespace") == prefix.split("/")[0]
        ]
        emit(attributes)
    elif words[2] == "get-serial-port-output":
        print("fake serial port output")
elif words[:2] == ["compute", "disks"] and words[2] == "describe":
    disk = STATE.get("disks", {}).get(name)
    emit(disk) if disk is not None else missing(f"disk {name}")
elif words[:2] == ["compute", "addresses"] and words[2] == "describe":
    address = STATE.get("addresses", {}).get(name)
    emit(address) if address is not None else missing(f"address {name}")
elif words[:2] == ["compute", "instance-templates"] and words[2] == "describe":
    template = STATE.get("instance_templates", {}).get(name)
    emit(template) if template is not None else missing(f"instance template {name}")
elif words[:3] == ["compute", "instance-groups", "managed"] and len(words) > 4:
    mig = STATE.get("migs", {}).get(words[4])
    if words[3] == "describe":
        emit(mig) if mig is not None else missing(f"instance group {words[4]}")
    elif words[3] == "list-instances":
        emit(STATE.get("mig_instances", {}).get(words[4], []))
elif words[:3] == ["iam", "service-accounts", "describe"]:
    if name not in STATE.get("service_accounts", [name]):
        missing(f"service account {name}")
elif words[:2] == ["secrets", "describe"]:
    if words[2] not in STATE.get("secrets", {}):
        missing(f"secret {words[2]}")
elif words[:3] == ["secrets", "versions", "access"]:
    versions = STATE.get("secrets", {}).get(flag("secret"))
    if not versions:
        missing(f"secret version of {flag('secret')}")
    sys.stdout.write(versions[-1])
elif words[:3] == ["secrets", "versions", "list"]:
    versions = STATE.get("secrets", {}).get(name, [])
    emit([{"name": f"projects/p/secrets/{name}/versions/{i + 1}", "state": "ENABLED"} for i in range(len(versions))])
elif words[:2] == ["projects", "get-iam-policy"]:
    emit(STATE.get("project_iam_policy", {"bindings": []}))
PY
chmod +x "${FAKE_BIN}/gcloud"

# --- fake bb: the wrapper runs it under `env -i`, so it writes next to the
# allowlisted TMPDIR: its argv, then the contents of every --bazelrc file.
cat >"${FAKE_BIN}/bb" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$@" >"${TMPDIR}/fake-bb.argv"
: >"${TMPDIR}/fake-bb.rc"
for arg in "$@"; do
  case "${arg}" in
    --bazelrc=*) [[ -f "${arg#--bazelrc=}" ]] && cat "${arg#--bazelrc=}" >>"${TMPDIR}/fake-bb.rc" ;;
  esac
done
echo "INFO: Elapsed time: 0.1s" >&2
SH
chmod +x "${FAKE_BIN}/bb"

new_state() {
  local dir="${TEST_ROOT}/gcloud-$1"
  mkdir -p "${dir}"
  printf '%s\n' "$2" >"${dir}/state.json"
  : >"${dir}/calls.jsonl"
  printf '%s\n' "${dir}"
}

# Prints one line per recorded gcloud call: the argv joined by spaces.
calls() {
  python3 -c 'import json,sys
for line in open(sys.argv[1]):
    print(" ".join(json.loads(line)["argv"]))' "$1/calls.jsonl"
}

# stdin bodies the fake received, one per line, as "secret-name<TAB>value".
secret_writes() {
  python3 -c 'import json,sys
for line in open(sys.argv[1]):
    call = json.loads(line)
    if "stdin" in call:
        words = [a for a in call["argv"] if not a.startswith("--")]
        print(words[3] + "\t" + call["stdin"])' "$1/calls.jsonl"
}

# --- scripts/buildbuddy-bazel-poc -------------------------------------------

# run_wrapper NAME [VAR=value ...] -- [wrapper args ...]
run_wrapper() {
  local name="$1"
  shift
  local work="${TEST_ROOT}/wrapper-${name}"
  local -a assignments=()
  while [[ $# -gt 0 && "$1" != "--" ]]; do
    assignments+=("$1")
    shift
  done
  [[ $# -gt 0 ]] && shift
  mkdir -p "${work}/home" "${work}/tmp" "${work}/cache"
  : >"${work}/secrets.env"
  set +e
  (
    cd "${REPO_ROOT}" &&
      env -i \
        PATH="${PATH}" HOME="${work}/home" TMPDIR="${work}/tmp" XDG_CACHE_HOME="${work}/cache" \
        MEERKAT_SECRETS_ENV="${work}/secrets.env" MEERKAT_IMPORT_LOGIN_ZSH_ENV=0 \
        BUILDBUDDY_BB="${FAKE_BIN}/bb" BUILDBUDDY_API_KEY="${FAKE_API_KEY}" \
        BUILDBUDDY_NO_DEFAULT_OUTPUT_BASE=1 BUILDBUDDY_NO_DEFAULT_REPOSITORY_CACHE=1 \
        BUILDBUDDY_NO_CACHE_REPAIR=1 BUILDBUDDY_MAX_EXIT_LAG_SECS=0 \
        "${assignments[@]}" \
        scripts/buildbuddy-bazel-poc "$@"
  ) >"${work}/out" 2>&1
  wrapper_status=$?
  set -e
  wrapper_work="${work}"
}

expect_refused() {
  local label="$1"
  if [[ "${wrapper_status}" -eq 0 ]]; then
    fail "wrapper sent the API key to a non-TLS endpoint (${label})"
  elif ! grep -q 'refusing to send the BuildBuddy API key to non-TLS endpoint' "${wrapper_work}/out"; then
    fail "wrapper failed for another reason (${label}): $(tail -n 3 "${wrapper_work}/out")"
  elif [[ -e "${wrapper_work}/tmp/fake-bb.argv" ]]; then
    fail "wrapper started the client before refusing (${label})"
  else
    pass "wrapper refuses the API key for ${label}"
  fi
}

expect_ran() {
  local label="$1"
  shift
  if [[ "${wrapper_status}" -ne 0 || ! -s "${wrapper_work}/tmp/fake-bb.argv" ]]; then
    fail "wrapper did not run the client (${label}): $(tail -n 3 "${wrapper_work}/out")"
    return
  fi
  local needle
  for needle in "$@"; do
    if ! grep -qxF -- "${needle}" "${wrapper_work}/tmp/fake-bb.argv"; then
      fail "wrapper argv lacks ${needle} (${label})"
      return
    fi
  done
  if grep -qF -- "${FAKE_API_KEY}" "${wrapper_work}/tmp/fake-bb.argv"; then
    fail "the API key is in the client argv (${label})"
  elif ! grep -qxF -- "build --remote_header=x-buildbuddy-api-key=${FAKE_API_KEY}" "${wrapper_work}/tmp/fake-bb.rc"; then
    fail "the API key is not in the private bazelrc (${label})"
  else
    pass "wrapper runs ${label} with the key only in its private bazelrc"
  fi
}

run_wrapper gcp-plaintext MEERKAT_BAZEL_BACKEND=gcp-buildbuddy BUILDBUDDY_BAZEL_CONFIG=buildbuddy-linux-gcp-ci-rbe \
  BUILDBUDDY_GRPC_URL=grpc://10.20.30.40:1985 --
expect_refused "a plaintext grpc:// self-hosted endpoint"

run_wrapper gcp-http MEERKAT_BAZEL_BACKEND=gcp-buildbuddy BUILDBUDDY_BAZEL_CONFIG=buildbuddy-linux-gcp-ci-rbe \
  BUILDBUDDY_ENDPOINT=http://10.20.30.40:8080 --
expect_refused "an http:// endpoint (normalized to grpc://)"

run_wrapper flag-plaintext -- --remote_cache=grpc://cache.example.invalid:9092
expect_refused "an explicit --remote_cache=grpc:// flag"

run_wrapper flag-split -- --bes_backend grpc://bes.example.invalid:1985
expect_refused "an explicit two-token --bes_backend grpc:// flag"

run_wrapper hosted --
expect_ran "the hosted default (grpcs:// from .bazelrc)" "--config=buildbuddy"

run_wrapper gcp-tls MEERKAT_BAZEL_BACKEND=gcp-buildbuddy BUILDBUDDY_BAZEL_CONFIG=buildbuddy-linux-gcp-ci-rbe \
  BUILDBUDDY_GRPC_URL=grpcs://127.0.0.1:1986 BUILDBUDDY_HTTP_URL=https://localhost:8081 --
expect_ran "a grpcs:// self-hosted endpoint" "--remote_cache=grpcs://127.0.0.1:1986" \
  "--remote_executor=grpcs://127.0.0.1:1986" "--bes_backend=grpcs://127.0.0.1:1986"

# --- scripts/gcp-buildbuddy-executor-pool sync-secrets ------------------------

state="$(new_state sync-secrets '{"secrets": {"buildbuddy-endpoint": ["grpc://203.0.113.9:1985"], "buildbuddy-api-key": ["cloud-key-value"]}}')"
set +e
FAKE_GCLOUD_STATE="${state}" PATH="${FAKE_BIN}:${PATH}" \
  BUILDBUDDY_GRPC_URL=grpcs://10.20.30.40:1986 BUILDBUDDY_API_KEY=cloud-key-value \
  "${REPO_ROOT}/scripts/gcp-buildbuddy-executor-pool" sync-secrets >"${state}/out" 2>&1
sync_status=$?
set -e
if [[ "${sync_status}" -ne 0 ]]; then
  fail "sync-secrets failed: $(tail -n 3 "${state}/out")"
elif calls "${state}" | grep -q 'buildbuddy-api-key'; then
  fail "sync-secrets touched the executor key secret: $(calls "${state}" | grep 'buildbuddy-api-key' | head -n 1)"
elif secret_writes "${state}" | grep -qF 'cloud-key-value'; then
  fail "sync-secrets wrote the API key value into Secret Manager"
elif [[ "$(secret_writes "${state}")" != "$(printf 'buildbuddy-endpoint\tgrpcs://10.20.30.40:1986')" ]]; then
  fail "sync-secrets wrote unexpected secrets: $(secret_writes "${state}")"
else
  pass "sync-secrets writes only the endpoint secret and never the executor key"
fi

# --- .github/workflows/buildbuddy.yml uses its own key ------------------------

workflow="${REPO_ROOT}/.github/workflows/buildbuddy.yml"
if grep -q 'secrets\.BUILDBUDDY_API_KEY' "${workflow}"; then
  fail "buildbuddy.yml reads the BuildBuddy Cloud key (secrets.BUILDBUDDY_API_KEY)"
elif ! grep -qF 'BUILDBUDDY_API_KEY: ${{ secrets.MEERKAT_GCP_BUILDBUDDY_API_KEY }}' "${workflow}"; then
  fail "buildbuddy.yml does not take its API key from secrets.MEERKAT_GCP_BUILDBUDDY_API_KEY"
else
  pass "buildbuddy.yml uses the self-hosted key, never the BuildBuddy Cloud key"
fi

if [[ "${failures}" -gt 0 ]]; then
  echo "${failures} failure(s)" >&2
  exit 1
fi
echo "PASS: self-hosted GCP BuildBuddy hardening contract"
