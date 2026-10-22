#!/usr/bin/env bash
# Contract test for the self-hosted GCP BuildBuddy path: what the scripts
# would send to GCP and to BuildBuddy, checked against stubs. `gcloud` and
# `bb` are fakes on PATH that record their arguments; nothing here reaches
# GCP, BuildBuddy or any other endpoint.
#
#   control plane (scripts/gcp-buildbuddy-control-plane)
#     - the rendered config: no anonymous use, an auth provider, a secret
#       JWT key, TLS, executor authorization, user-owned executors; the
#       startup script installs exactly that config and its hash;
#     - ensure creates only the deny rule, the IAP rule and the executor rule
#       (no 0.0.0.0/0 allow, no static address, no network tag), and refuses,
#       changing nothing, on the legacy rule (open or disabled), a disabled
#       or drifted managed rule, a foreign rule opening the VM, a VM or data
#       disk not made from this rendering, or an image without a digest;
#       it never re-enables or edits a firewall rule;
#     - verify, endpoint, tunnel and stop.
#   executor pool (scripts/gcp-buildbuddy-executor-pool)
#     - sync-secrets accepts only grpcs:// to a private IPv4 plus the CA and
#       never writes the executor key; per-secret IAM; digest-pinned images;
#       derived template names, refusing a drifted template or stale
#       instances.
#   Bazel wrapper (scripts/buildbuddy-bazel-poc)
#     - never sends the API key to a non-TLS endpoint, keeps it out of argv,
#       and for gcp-buildbuddy trusts the control plane CA and pins the
#       action image by digest.
#   workflow wiring (buildbuddy.yml and the lane batch action)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/meerkat-gcp-bb-hardening.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

FAKE_BIN="${TEST_ROOT}/bin"
mkdir -p "${FAKE_BIN}"
FAKE_API_KEY="fake-buildbuddy-api-key-0123456789"
FAKE_CI_IMAGE="europe-west1-docker.pkg.dev/proj-test/meerkat-ci/meerkat-ci-rust:1.94.0@sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
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
# <state>/calls.jsonl. Mutating verbs only record. start-iap-tunnel accepts
# one connection on its --local-host-port and exits.
cat >"${FAKE_BIN}/gcloud" <<'PY'
#!/usr/bin/env python3
import json
import os
import re
import socket
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

record = {"argv": ARGV, "words": positionals}
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
        emit([item for item in STATE.get("guest_attributes", {}).get(name, [])
              if not prefix or item.get("namespace") == prefix.split("/")[0]])
    elif words[2] == "get-serial-port-output":
        print("fake serial port output")
elif words[:2] == ["compute", "start-iap-tunnel"]:
    host, _, port = (flag("local-host-port") or "").rpartition(":")
    server = socket.socket()
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind((host, int(port)))
    server.listen(1)
    server.settimeout(30)
    try:
        server.accept()[0].close()
    except OSError:
        pass
    server.close()
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

# --- scripts/buildbuddy-bazel-poc -------------------------------------------

# run_wrapper NAME [VAR=value ...] -- [wrapper args ...]
# Runs the wrapper of ${wrapper_root} (the repository unless a case points it
# at a scratch workspace with its own .bazelrc).
wrapper_root="${REPO_ROOT}"
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
    cd "${wrapper_root}" &&
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
  local label="$1" message="$2"
  if [[ "${wrapper_status}" -eq 0 ]]; then
    fail "wrapper ran with ${label}"
  elif ! grep -qF -- "${message}" "${wrapper_work}/out"; then
    fail "wrapper failed for another reason (${label}): $(tail -n 3 "${wrapper_work}/out")"
  elif [[ -e "${wrapper_work}/tmp/fake-bb.argv" ]]; then
    fail "wrapper started the client before refusing (${label})"
  else
    pass "wrapper refuses ${label}"
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

NON_TLS="refusing to send the BuildBuddy API key to non-TLS or unscannable endpoint configuration"
GCP=(MEERKAT_BAZEL_BACKEND=gcp-buildbuddy BUILDBUDDY_BAZEL_CONFIG=buildbuddy-linux-gcp-ci-rbe)
printf '%s\n' "-----BEGIN CERTIFICATE-----" "fake" "-----END CERTIFICATE-----" >"${TEST_ROOT}/control-ca.pem"

run_wrapper gcp-plaintext "${GCP[@]}" BUILDBUDDY_GRPC_URL=grpc://10.20.30.40:1985 --
expect_refused "the API key for a plaintext grpc:// self-hosted endpoint" "${NON_TLS}"

run_wrapper gcp-http "${GCP[@]}" BUILDBUDDY_ENDPOINT=http://10.20.30.40:8080 --
expect_refused "the API key for an http:// endpoint (normalized to grpc://)" "${NON_TLS}"

run_wrapper flag-plaintext -- --remote_cache=grpc://cache.example.invalid:9092
expect_refused "the API key for an explicit --remote_cache=grpc:// flag" "${NON_TLS}"

run_wrapper flag-split -- --bes_backend grpc://bes.example.invalid:1985
expect_refused "the API key for an explicit two-token --bes_backend grpc:// flag" "${NON_TLS}"

run_wrapper hosted --
expect_ran "the hosted default (grpcs:// from .bazelrc)" "--config=buildbuddy"

# A scratch copy of the repository whose .bazelrc gets one extra line.
scratch_workspace() {
  local name="$1" line="$2"
  local ws="${TEST_ROOT}/workspace-${name}"
  mkdir -p "${ws}"
  cp -R "${REPO_ROOT}/scripts" "${ws}/scripts"
  cp "${REPO_ROOT}/.bazelrc" "${REPO_ROOT}/MODULE.bazel.lock" "${REPO_ROOT}/.bazelversion" "${ws}/"
  printf '%s\n' "${line}" >>"${ws}/.bazelrc"
  git -C "${ws}" init -q
  wrapper_root="${ws}"
}

scratch_workspace mixed-line "build:buildbuddy --remote_cache=grpc://plain.example.invalid:9092 --bes_backend=grpcs://tls.example.invalid"
run_wrapper rc-mixed-line --
expect_refused "a .bazelrc line with a plaintext endpoint before a TLS one" "grpc://plain.example.invalid:9092"
scratch_workspace split-line "build:buildbuddy --bes_backend grpcs://tls.example.invalid --remote_executor grpc://plain.example.invalid:1985"
run_wrapper rc-split-line --
expect_refused "a .bazelrc line with a two-token plaintext endpoint after a TLS one" "grpc://plain.example.invalid:1985"
scratch_workspace import-line "try-import %workspace%/user.bazelrc"
run_wrapper rc-import --
expect_refused "a .bazelrc that imports an unscanned rc file" "imported rc files are not scanned"
scratch_workspace quoted-line "build:buildbuddy \"--remote_cache=grpc://plain.example.invalid:9092\""
run_wrapper rc-quoted --
expect_refused "a .bazelrc line with a quoted endpoint flag" "quoted endpoint flags are not scanned"
scratch_workspace tls-line "build:buildbuddy --remote_cache=grpcs://a.example.invalid --bes_backend=grpcs://b.example.invalid"
run_wrapper rc-tls-line --
expect_ran "a .bazelrc line with only TLS endpoints" "--config=buildbuddy"
wrapper_root="${REPO_ROOT}"

run_wrapper gcp-no-ca "${GCP[@]}" BUILDBUDDY_GRPC_URL=grpcs://127.0.0.1:1986 \
  MEERKAT_BUILDBUDDY_CI_IMAGE="${FAKE_CI_IMAGE}" --
expect_refused "gcp-buildbuddy without the control plane CA" "BUILDBUDDY_TLS_CERTIFICATE must name the control plane's CA"

run_wrapper gcp-tag-image "${GCP[@]}" BUILDBUDDY_GRPC_URL=grpcs://127.0.0.1:1986 \
  BUILDBUDDY_TLS_CERTIFICATE="${TEST_ROOT}/control-ca.pem" \
  MEERKAT_BUILDBUDDY_CI_IMAGE=europe-west1-docker.pkg.dev/proj-test/meerkat-ci/meerkat-ci-rust:1.94.0 --
expect_refused "gcp-buildbuddy with an action image tag without a digest" "MEERKAT_BUILDBUDDY_CI_IMAGE must pin the action image by digest"

run_wrapper gcp-tls "${GCP[@]}" BUILDBUDDY_GRPC_URL=grpcs://127.0.0.1:1986 BUILDBUDDY_HTTP_URL=https://localhost:8081 \
  BUILDBUDDY_TLS_CERTIFICATE="${TEST_ROOT}/control-ca.pem" MEERKAT_BUILDBUDDY_CI_IMAGE="docker://${FAKE_CI_IMAGE}" --
expect_ran "gcp-buildbuddy over its IAP tunnel" "--remote_cache=grpcs://127.0.0.1:1986" \
  "--remote_executor=grpcs://127.0.0.1:1986" "--bes_backend=grpcs://127.0.0.1:1986" \
  "--tls_certificate=${TEST_ROOT}/control-ca.pem" \
  "--remote_exec_header=x-buildbuddy-platform.container-image=docker://${FAKE_CI_IMAGE}" \
  "--remote_exec_header=x-buildbuddy-platform.use-self-hosted-executors=true"

# --- control plane and executor pool scenarios --------------------------------

cat >"${TEST_ROOT}/scenarios.py" <<'PY'
import base64
import datetime
import hashlib
import json
import os
import re
import socket
import subprocess
import sys

REPO, FAKE_BIN, ROOT = sys.argv[1:4]
CONTROL = os.path.join(REPO, "scripts", "gcp-buildbuddy-control-plane")
POOL = os.path.join(REPO, "scripts", "gcp-buildbuddy-executor-pool")
PROJECT = "proj-test"
CONTROL_SA = f"meerkat-bb-control@{PROJECT}.iam.gserviceaccount.com"
EXECUTOR_SA = f"meerkat-bb-executor@{PROJECT}.iam.gserviceaccount.com"
CONTROL_IMAGE = "buildbuddy.bbcr.io/public/buildbuddy-app-enterprise:enterprise-v2.265.0@sha256:" + "a" * 64
EXECUTOR_IMAGE = "buildbuddy.bbcr.io/public/buildbuddy-executor-enterprise:enterprise-v2.265.0@sha256:" + "b" * 64
CI_IMAGE = "europe-west1-docker.pkg.dev/proj-test/meerkat-ci/meerkat-ci-rust:1.94.0@sha256:" + "c" * 64
CA_PEM = "-----BEGIN CERTIFICATE-----\nZmFrZSBjb250cm9sLXBsYW5lIENB\n-----END CERTIFICATE-----\n"
CA_B64 = base64.b64encode(CA_PEM.encode()).decode()
FAILURES = []
ALL_CALLS = []
COUNTER = [0]


def fail(message):
    FAILURES.append(message)
    print(f"FAIL: {message}", file=sys.stderr)


def ok(message):
    print(f"ok: {message}")


def run(script, args, state, cwd=None, **extra):
    COUNTER[0] += 1
    directory = os.path.join(ROOT, f"scenario-{COUNTER[0]}")
    os.makedirs(directory)
    with open(os.path.join(directory, "state.json"), "w", encoding="utf-8") as handle:
        json.dump(state, handle)
    open(os.path.join(directory, "calls.jsonl"), "w").close()
    env = {key: os.environ[key] for key in ("PATH", "HOME", "LANG") if key in os.environ}
    env["PATH"] = FAKE_BIN + os.pathsep + env["PATH"]
    env.update(FAKE_GCLOUD_STATE=directory, MEERKAT_GCP_PROJECT_ID=PROJECT,
               MEERKAT_GCP_ZONE="europe-west1-b", TMPDIR=directory)
    env.update({key: str(value) for key, value in extra.items()})
    proc = subprocess.run([script] + args, env=env, cwd=cwd, capture_output=True, text=True, timeout=120)
    with open(os.path.join(directory, "calls.jsonl"), encoding="utf-8") as handle:
        calls = [json.loads(line) for line in handle]
    ALL_CALLS.extend(calls)
    return proc, calls


def flag_values(call, name):
    argv = call["argv"]
    values = []
    for index, token in enumerate(argv):
        if token == f"--{name}" and index + 1 < len(argv):
            values.append(argv[index + 1])
        elif token.startswith(f"--{name}="):
            values.append(token.split("=", 1)[1])
    return values


def flag(call, name):
    values = flag_values(call, name)
    return values[-1] if values else None


def find(calls, *words):
    return [call for call in calls if call["words"][:len(words)] == list(words)]


MUTATING_VERBS = {"create", "update", "delete", "start", "stop", "resume", "resize",
                  "add-iam-policy-binding", "remove-iam-policy-binding", "set-instance-template"}


def mutating(calls):
    return [" ".join(call["argv"]) for call in calls
            if MUTATING_VERBS & set(call["words"][:4]) or call["words"][:3] == ["secrets", "versions", "add"]]


def rule(name, action="ALLOW", perms=(("tcp", ["22"]),), sources=None, source_sas=None,
         target_sas=None, target_tags=None, priority=1000, disabled=False):
    body = {
        "name": name,
        "network": f"https://www.googleapis.com/compute/v1/projects/{PROJECT}/global/networks/default",
        "direction": "INGRESS",
        "priority": priority,
        "disabled": disabled,
        ("allowed" if action == "ALLOW" else "denied"): [
            dict({"IPProtocol": proto}, **({"ports": ports} if ports else {})) for proto, ports in perms
        ],
    }
    for key, value in (("sourceRanges", sources), ("sourceServiceAccounts", source_sas),
                       ("targetServiceAccounts", target_sas), ("targetTags", target_tags)):
        if value:
            body[key] = value
    return body


DENY = rule("meerkat-bb-control-deny-ingress", action="DENY", perms=(("all", None),),
            sources=["0.0.0.0/0"], target_sas=[CONTROL_SA], priority=1000)
IAP = rule("meerkat-bb-control-allow-iap", perms=(("tcp", ["22", "1986", "8081"]),),
           sources=["35.235.240.0/20"], target_sas=[CONTROL_SA], priority=900)
EXECUTORS = rule("meerkat-bb-control-allow-executors", perms=(("tcp", ["1986"]),),
                 source_sas=[EXECUTOR_SA], target_sas=[CONTROL_SA], priority=900)
MANAGED = [DENY, IAP, EXECUTORS]
DEFAULTS = [
    rule("default-allow-ssh", perms=(("tcp", ["22"]),), sources=["0.0.0.0/0"], priority=65534),
    rule("default-allow-internal", perms=(("tcp", ["0-65535"]), ("udp", ["0-65535"]), ("icmp", None)),
         sources=["10.128.0.0/9"], priority=65534),
]
LEGACY = rule("meerkat-bb-control-grpc-http", perms=(("tcp", ["8080", "1985"]),),
              sources=["0.0.0.0/0"], target_tags=["meerkat-bb-control"])
DISK_OK = {"name": "meerkat-bb-control-data",
           "labels": {"purpose": "buildbuddy-control", "repo": "meerkat", "bb-baseline": "hardened-v1"}}
LAST_START = "2026-10-14T10:00:00.000-07:00"
LAST_START_EPOCH = int(datetime.datetime.fromisoformat(LAST_START).timestamp())


def instance(script, status="TERMINATED", account=CONTROL_SA, ip="10.132.0.7", guest="TRUE"):
    return {
        "name": "meerkat-bb-control", "status": status, "lastStartTimestamp": LAST_START,
        "serviceAccounts": [{"email": account}],
        "networkInterfaces": [{"networkIP": ip, "accessConfigs": [{"natIP": "34.1.2.3"}]}],
        "metadata": {"items": [{"key": "startup-script", "value": script},
                               {"key": "enable-guest-attributes", "value": guest}]},
    }


def attributes(ready="ok", config=None, booted=LAST_START_EPOCH + 5, ca=CA_B64):
    values = {"ready": ready, "config-sha256": config, "boot-started": None if booted is None else str(booted), "tls-ca": ca}
    return [{"namespace": "meerkat", "key": key, "value": value} for key, value in values.items() if value is not None]


CONTROL_ENV = {"MEERKAT_GCP_BUILDBUDDY_CONTROL_IMAGE": CONTROL_IMAGE}

# --- rendering ---------------------------------------------------------------

proc, _ = run(CONTROL, ["render-config"], {})
config = proc.stdout
want_lines = [
    '  build_buddy_url: "https://localhost:8081"',
    "  enable_anonymous_usage: false",
    "  enable_self_auth: true",
    '  jwt_key: "${BB_AUTH_JWT_KEY}"',
    "  enable_ssl: true",
    "  enable_remote_exec: true",
    "  enable_user_owned_executors: true",
    "  require_executor_authorization: true",
]
missing_lines = [line for line in want_lines if line not in config.splitlines()]
plaintext = [line for line in config.splitlines() if re.search(r"\b(grpc|http)://", line)]
if proc.returncode != 0 or missing_lines or plaintext or "enable_anonymous_usage: true" in config:
    fail(f"rendered config: missing {missing_lines}, plaintext {plaintext}")
else:
    ok("rendered config: no anonymous use, self auth, per-boot JWT key, TLS, executor authorization")
proc, _ = run(CONTROL, ["config-sha256"], {})
config_sha = proc.stdout.strip()
if config_sha != hashlib.sha256(config.encode()).hexdigest():
    fail("config-sha256 is not the sha256 of render-config")
proc_a, _ = run(CONTROL, ["render-config"], {})
if proc_a.stdout != config:
    fail("render-config is not deterministic")

for image in ("", "buildbuddy.bbcr.io/public/buildbuddy-app-enterprise:latest",
              "buildbuddy.bbcr.io/public/buildbuddy-app-enterprise:enterprise-v2.265.0"):
    proc, calls = run(CONTROL, ["render-startup-script"], {}, MEERKAT_GCP_BUILDBUDDY_CONTROL_IMAGE=image)
    if proc.returncode == 0 or "pin the BuildBuddy app image by digest" not in proc.stderr:
        fail(f"render-startup-script accepted the unpinned image {image!r}")
proc, calls = run(CONTROL, ["ensure"], {"firewall_rules": DEFAULTS})
if proc.returncode == 0 or calls:
    fail(f"ensure without a pinned image ran gcloud: {[' '.join(c['argv']) for c in calls]}")
else:
    ok("an app image without a digest is refused before any gcloud call")

proc, _ = run(CONTROL, ["render-startup-script"], {}, **CONTROL_ENV)
STARTUP = proc.stdout
lines = STARTUP.splitlines()
try:
    begin = lines.index("cat >/etc/buildbuddy/config.yaml <<'BUILDBUDDY_CONFIG'")
    end = lines.index("BUILDBUDDY_CONFIG", begin + 1)
    installed = "\n".join(lines[begin + 1:end]) + "\n"
except ValueError:
    installed = None
checks = {
    "pins the image by digest": f'IMAGE="{CONTROL_IMAGE}"' in lines,
    "embeds the config hash": f'CONFIG_SHA256="{config_sha}"' in lines,
    "installs exactly the rendered config": installed == config,
    "refuses a differing installed config": "config-mismatch" in STARTUP,
    "signs a per-boot certificate for the tunnel and internal IP": "subjectAltName=DNS:localhost,IP:127.0.0.1,IP:${internal_ip}" in STARTUP,
    "destroys the CA key": 'rm -f "${tls}/ca.key"' in STARTUP,
    "checks readiness over TLS with the CA": '--cacert "${tls}/ca.crt" "https://127.0.0.1:${HTTPS_PORT}/readyz"' in STARTUP,
    "reports the config hash and CA": "report config-sha256" in STARTUP and "report tls-ca" in STARTUP,
    "generates a per-boot JWT key": "BB_AUTH_JWT_KEY=" in STARTUP and "/dev/urandom" in STARTUP,
}
broken = [name for name, good in checks.items() if not good]
if broken:
    fail(f"startup script: {broken}")
else:
    ok("startup script installs the rendered config byte for byte and serves per-boot TLS")

# Run the startup script's TLS block itself when openssl is present.
first = next((i for i, l in enumerate(lines) if l.startswith('internal_ip="$(metadata')), None)
last = next((i for i, l in enumerate(lines) if l.startswith("cat /etc/ssl/certs/ca-certificates.crt")), None)
if first is None or last is None:
    fail("startup script has no per-boot TLS block")
elif subprocess.run(["sh", "-c", "command -v openssl"], capture_output=True).returncode == 0:
    state = os.path.join(ROOT, "tls-run")
    block = "\n".join(lines[first:last]).replace("state=/run/meerkat-bb-control", f"state={state}")
    script = "set -euo pipefail\nmetadata() { echo 10.132.0.7; }\n" + block + "\n"
    proc = subprocess.run(["bash", "-c", script], capture_output=True, text=True)
    tls = os.path.join(state, "tls")
    verified = proc.returncode == 0 and all(
        subprocess.run(["openssl", "verify", "-CAfile", os.path.join(tls, "ca.crt"), *opt,
                        os.path.join(tls, "server.crt")], capture_output=True).returncode == 0
        for opt in (["-verify_ip", "127.0.0.1"], ["-verify_ip", "10.132.0.7"], ["-verify_hostname", "localhost"]))
    wrong = subprocess.run(["openssl", "verify", "-CAfile", os.path.join(tls, "ca.crt"), "-verify_ip",
                            "34.1.2.3", os.path.join(tls, "server.crt")], capture_output=True).returncode
    if not verified or wrong == 0 or os.path.exists(os.path.join(tls, "ca.key")):
        fail(f"per-boot TLS block: verified={verified}, foreign IP rejected={wrong != 0}, "
             f"CA key destroyed={not os.path.exists(os.path.join(tls, 'ca.key'))}: {proc.stderr[-300:]}")
    else:
        ok("per-boot certificate verifies for 127.0.0.1, localhost and the internal IP only; CA key destroyed")

# --- ensure: fresh project ------------------------------------------------------

proc, calls = run(CONTROL, ["ensure"], {"firewall_rules": DEFAULTS}, **CONTROL_ENV)
creates = find(calls, "compute", "firewall-rules", "create")
names = [call["words"][3] for call in creates]
expected_rules = {
    "meerkat-bb-control-deny-ingress": {"action": "DENY", "rules": "all", "source-ranges": "0.0.0.0/0",
                                        "priority": "1000", "direction": "INGRESS", "network": "default",
                                        "target-service-accounts": CONTROL_SA},
    "meerkat-bb-control-allow-iap": {"action": "ALLOW", "rules": "tcp:22,tcp:1986,tcp:8081",
                                     "source-ranges": "35.235.240.0/20", "priority": "900",
                                     "direction": "INGRESS", "network": "default",
                                     "target-service-accounts": CONTROL_SA},
    "meerkat-bb-control-allow-executors": {"action": "ALLOW", "rules": "tcp:1986",
                                           "source-service-accounts": EXECUTOR_SA, "priority": "900",
                                           "direction": "INGRESS", "network": "default",
                                           "target-service-accounts": CONTROL_SA},
}
problems = []
if proc.returncode != 0:
    problems.append(f"exit {proc.returncode}: {proc.stderr[-400:]}")
if names != list(expected_rules):
    problems.append(f"firewall creates {names}, expected the deny rule first then {list(expected_rules)[1:]}")
for call in creates:
    want = expected_rules.get(call["words"][3], {})
    for key, value in want.items():
        if flag(call, key) != value:
            problems.append(f"{call['words'][3]} --{key}={flag(call, key)!r}, expected {value!r}")
    if flag_values(call, "allow") or flag_values(call, "target-tags"):
        problems.append(f"{call['words'][3]} uses --allow or --target-tags")
    if flag(call, "action") == "ALLOW" and "0.0.0.0/0" in " ".join(call["argv"]):
        problems.append(f"{call['words'][3]} allows 0.0.0.0/0")
    if call["words"][3] == "meerkat-bb-control-allow-executors" and flag_values(call, "source-ranges"):
        problems.append("the executor rule also takes source ranges")
disks = find(calls, "compute", "disks", "create")
if len(disks) != 1 or "bb-baseline=hardened-v1" not in (flag(disks[0], "labels") or ""):
    problems.append("the data disk is not created with the baseline label")
created = find(calls, "compute", "instances", "create")
if len(created) != 1:
    problems.append("no instance create")
else:
    vm = created[0]
    if flag_values(vm, "address") or flag_values(vm, "tags"):
        problems.append("the VM gets a static address or network tags")
    if flag(vm, "metadata") != "enable-guest-attributes=TRUE":
        problems.append("the VM does not enable guest attributes")
    if flag(vm, "service-account") != CONTROL_SA or flag(vm, "network") != "default":
        problems.append("the VM service account or network differs")
    if (vm.get("files") or {}).get("startup-script") != STARTUP:
        problems.append("the VM startup script is not the rendered one")
if find(calls, "compute", "addresses") or find(calls, "compute", "firewall-rules", "update"):
    problems.append("ensure reserved an address or updated a firewall rule")
if problems:
    fail("ensure on a fresh project: " + "; ".join(problems))
else:
    ok("ensure creates the deny rule, then the IAP and executor rules, the baseline disk and a tagless VM with the rendered startup script")

# --- ensure: refusals change nothing ----------------------------------------------

drifted_iap = dict(IAP, sourceRanges=["0.0.0.0/0"])
disabled_iap = dict(IAP, disabled=True)
old_vm = instance("#!/usr/bin/env bash\necho pre-hardening control plane\n")
unbaselined_disk = {"name": "meerkat-bb-control-data", "labels": {"purpose": "buildbuddy-control"}}
refusals = [
    ("the legacy rule open to 0.0.0.0/0", {"firewall_rules": DEFAULTS + MANAGED + [LEGACY]},
     "legacy firewall rule meerkat-bb-control-grpc-http exists"),
    ("the legacy rule disabled", {"firewall_rules": DEFAULTS + MANAGED + [dict(LEGACY, disabled=True)]},
     "it is never reused or re-enabled"),
    ("a disabled managed rule", {"firewall_rules": DEFAULTS + [DENY, disabled_iap, EXECUTORS]},
     "a disabled rule is never re-enabled"),
    ("a managed rule opened to 0.0.0.0/0", {"firewall_rules": DEFAULTS + [DENY, drifted_iap, EXECUTORS]},
     "managed firewall rule meerkat-bb-control-allow-iap sourceRanges"),
    ("a foreign rule opening the VM's service account",
     {"firewall_rules": DEFAULTS + MANAGED + [rule("debug-open", perms=(("tcp", ["8081"]),), sources=["0.0.0.0/0"],
                                                   target_sas=[CONTROL_SA])]},
     "firewall rule debug-open admits 0.0.0.0/0 to the control plane"),
    ("a foreign rule opening the legacy tag",
     {"firewall_rules": DEFAULTS + MANAGED + [rule("old-tag-open", sources=["::/0"], target_tags=["meerkat-bb-control"],
                                                   disabled=True)]},
     "firewall rule old-tag-open admits ::/0 to the control plane"),
    ("an open all-instance rule ahead of the deny rule",
     {"firewall_rules": DEFAULTS + MANAGED + [rule("all-open", perms=(("tcp", None),), sources=["0.0.0.0/0"], priority=500)]},
     "ahead of meerkat-bb-control-deny-ingress"),
    ("a narrower public range on the gRPCS port ahead of the deny",
     {"firewall_rules": DEFAULTS + MANAGED + [rule("partner-grpcs", perms=(("tcp", ["1986"]),), sources=["203.0.113.0/24"],
                                                   target_sas=[CONTROL_SA], priority=900)]},
     "firewall rule partner-grpcs lets 203.0.113.0/24 reach the control plane"),
    ("a single public address on every instance ahead of the deny",
     {"firewall_rules": DEFAULTS + MANAGED + [rule("office-ssh", perms=(("tcp", ["22"]),), sources=["198.51.100.7/32"], priority=500)]},
     "firewall rule office-ssh lets 198.51.100.7/32 reach the control plane"),
    ("the IAP range on a port outside the IAP allow",
     {"firewall_rules": DEFAULTS + MANAGED + [rule("iap-metrics", perms=(("tcp", ["9090"]),), sources=["35.235.240.0/20"],
                                                   target_sas=[CONTROL_SA], priority=900)]},
     "firewall rule iap-metrics lets 35.235.240.0/20 reach the control plane"),
    ("an IPv6 source the IPv4 deny does not shadow",
     {"firewall_rules": DEFAULTS + MANAGED + [rule("v6-open", perms=(("tcp", ["8081"]),), sources=["2001:db8::/32"], priority=65534)]},
     "firewall rule v6-open lets 2001:db8::/32 reach the control plane"),
    ("another service account on the gRPCS port ahead of the deny",
     {"firewall_rules": DEFAULTS + MANAGED + [rule("other-sa", perms=(("tcp", ["1986"]),), source_sas=[f"other@{PROJECT}.iam.gserviceaccount.com"],
                                                   target_sas=[CONTROL_SA], priority=900)]},
     "firewall rule other-sa lets other@"),
    ("a VM with network tags",
     {"firewall_rules": DEFAULTS + MANAGED, "instances": {"meerkat-bb-control": dict(instance(STARTUP), tags={"items": ["http-server"]})},
      "disks": {"meerkat-bb-control-data": DISK_OK}},
     "carries network tags (http-server)"),
    ("a VM made from another startup script",
     {"firewall_rules": DEFAULTS + MANAGED, "instances": {"meerkat-bb-control": old_vm}, "disks": {"meerkat-bb-control-data": DISK_OK}},
     "runs a startup script"),
    ("a data disk without the baseline label",
     {"firewall_rules": DEFAULTS + MANAGED, "disks": {"meerkat-bb-control-data": unbaselined_disk}},
     "lacks label bb-baseline=hardened-v1"),
]
for label, state, message in refusals:
    proc, calls = run(CONTROL, ["ensure"], state, **CONTROL_ENV)
    if proc.returncode == 0:
        fail(f"ensure accepted {label}")
    elif message not in proc.stderr or "nothing was changed" not in proc.stderr:
        fail(f"ensure refused {label} without saying so: {proc.stderr[-400:]}")
    elif mutating(calls):
        fail(f"ensure changed something before refusing {label}: {mutating(calls)}")
    else:
        ok(f"ensure refuses {label} and changes nothing")

# --- ensure: existing hardened resources -------------------------------------------

benign = DEFAULTS + MANAGED + [
    rule("all-open-shadowed", perms=(("tcp", None),), sources=["0.0.0.0/0"], priority=1000),
    rule("partner-shadowed", perms=(("tcp", ["443"]),), sources=["203.0.113.0/24"], priority=2000),
    rule("iap-ssh-everywhere", perms=(("tcp", ["22"]),), sources=["35.235.240.0/20"], priority=900),
    rule("executors-again", perms=(("tcp", ["1986"]),), source_sas=[EXECUTOR_SA], target_sas=[CONTROL_SA], priority=800),
    rule("other-target", perms=(("tcp", ["8081"]),), sources=["203.0.113.0/24"], target_sas=[f"other@{PROJECT}.iam.gserviceaccount.com"], priority=100),
]
proc, calls = run(CONTROL, ["ensure"], {"firewall_rules": benign,
                                        "instances": {"meerkat-bb-control": instance(STARTUP)},
                                        "disks": {"meerkat-bb-control-data": DISK_OK}}, **CONTROL_ENV)
changes = mutating(calls)
if proc.returncode != 0 or len(changes) != 1 or not changes[0].startswith("compute instances start meerkat-bb-control"):
    fail(f"ensure on the hardened plane: exit {proc.returncode}, changes {changes}: {proc.stderr[-300:]}")
else:
    ok("ensure only starts a stopped VM made from the rendering (shadowed, intended and unrelated rules are fine)")
proc, calls = run(CONTROL, ["ensure"], {"firewall_rules": MANAGED,
                                        "instances": {"meerkat-bb-control": instance(STARTUP, status="RUNNING")},
                                        "disks": {"meerkat-bb-control-data": DISK_OK}}, **CONTROL_ENV)
if proc.returncode != 0 or mutating(calls):
    fail(f"ensure on a running hardened plane changed {mutating(calls)}")

# --- verify ---------------------------------------------------------------------------

running = {"firewall_rules": MANAGED, "instances": {"meerkat-bb-control": instance(STARTUP, status="RUNNING")}}
for label, attrs, args, good, message in (
    ("the rendered config hash", attributes(config=config_sha), ["verify"], True, "verified meerkat-bb-control"),
    ("another config hash", attributes(config="0" * 64), ["verify"], False, "reports config sha256 '" + "0" * 64),
    ("a report from an earlier boot", attributes(config=config_sha, booted=LAST_START_EPOCH - 3600),
     ["verify", "--wait", "0"], False, "did not report ready"),
    ("a report from a boot just before this one", attributes(config=config_sha, booted=LAST_START_EPOCH - 30),
     ["verify", "--wait", "0"], False, "did not report ready"),
    ("a report without a boot time", attributes(config=config_sha, booted=None),
     ["verify", "--wait", "0"], False, "did not report ready"),
    ("a malformed boot time", attributes(config=config_sha, booted="1760461200.5"),
     ["verify", "--wait", "0"], False, "did not report ready"),
    ("a config mismatch", attributes(ready="config-mismatch", config=config_sha), ["verify"], False,
     "reports ready=config-mismatch"),
):
    proc, _ = run(CONTROL, args, dict(running, guest_attributes={"meerkat-bb-control": attrs}), **CONTROL_ENV)
    if (proc.returncode == 0) != good or message not in proc.stdout + proc.stderr:
        fail(f"verify with {label}: exit {proc.returncode}: {(proc.stdout + proc.stderr)[-300:]}")
    else:
        ok(f"verify {'accepts' if good else 'rejects'} {label}")
proc, _ = run(CONTROL, ["verify"], {"instances": {"meerkat-bb-control": old_vm},
                                    "guest_attributes": {"meerkat-bb-control": attributes(config=config_sha)}}, **CONTROL_ENV)
if proc.returncode == 0:
    fail("verify accepted a VM made from another startup script")

# --- endpoint, tunnel, stop --------------------------------------------------------------

output = os.path.join(ROOT, "github-output")
open(output, "w").close()
proc, _ = run(CONTROL, ["endpoint", "github-output"],
              dict(running, guest_attributes={"meerkat-bb-control": attributes(config=config_sha)}), GITHUB_OUTPUT=output)
with open(output, encoding="utf-8") as handle:
    published = handle.read()
want = ("executor_grpc_url=grpcs://10.132.0.7:1986\ngrpc_url=grpcs://127.0.0.1:1986\n"
        f"http_url=https://localhost:8081\ntls_ca_b64={CA_B64}\n")
if proc.returncode != 0 or published != want or "34.1.2.3" in published + proc.stdout:
    fail(f"endpoint github-output wrote {published!r}: {proc.stderr[-300:]}")
else:
    ok("endpoint publishes gRPCS to the internal IP and the tunnel, plus the CA, never the external IP")
proc, _ = run(CONTROL, ["endpoint"], {"instances": {"meerkat-bb-control": instance(STARTUP, ip="34.5.6.7")}})
if proc.returncode == 0:
    fail("endpoint accepted a public address")

probe = socket.socket()
probe.bind(("127.0.0.1", 0))
port = probe.getsockname()[1]
probe.close()
env_file = os.path.join(ROOT, "github-env")
open(env_file, "w").close()
proc, calls = run(CONTROL, ["tunnel"], {}, BUILDBUDDY_TLS_CA_B64=CA_B64, MEERKAT_GCP_BUILDBUDDY_TUNNEL_PORT=port,
                  GITHUB_ENV=env_file, RUNNER_TEMP=ROOT)
tunnels = find(calls, "compute", "start-iap-tunnel")
ca_file = os.path.join(ROOT, "meerkat-bb-control-ca.pem")
with open(env_file, encoding="utf-8") as handle:
    exported = handle.read()
if (proc.returncode != 0 or len(tunnels) != 1 or tunnels[0]["words"][2:4] != ["meerkat-bb-control", "1986"]
        or flag(tunnels[0], "local-host-port") != f"127.0.0.1:{port}"
        or exported != f"BUILDBUDDY_TLS_CERTIFICATE={ca_file}\n" or open(ca_file).read() != CA_PEM):
    fail(f"tunnel: exit {proc.returncode}, calls {[c['argv'] for c in tunnels]}, exported {exported!r}: {proc.stderr[-300:]}")
else:
    ok("tunnel forwards 127.0.0.1 to the gRPCS port over IAP and exports the CA for Bazel")
proc, _ = run(CONTROL, ["tunnel"], {})
if proc.returncode == 0:
    fail("tunnel ran without the control-plane CA")

proc, calls = run(CONTROL, ["stop"], running, MEERKAT_GCP_BUILDBUDDY_WAIT_ON_DOWN="0")
stops = find(calls, "compute", "instances", "stop")
if proc.returncode != 0 or len(stops) != 1 or "--async" not in stops[0]["argv"]:
    fail(f"stop: {[c['argv'] for c in stops]}")
else:
    ok("stop stops the VM (asynchronously in CI)")

# --- never re-enable ----------------------------------------------------------------------

source = open(CONTROL, encoding="utf-8").read()
forbidden = [token for token in ("firewall-rules update", "--no-disabled", "disabled=false", "firewall-rules delete")
             if token in source]
updates = [" ".join(c["argv"]) for c in ALL_CALLS if c["words"][:3] == ["compute", "firewall-rules", "update"]
           or c["words"][:3] == ["compute", "firewall-rules", "delete"]]
if forbidden or updates:
    fail(f"the control plane script can edit or re-enable firewall rules: {forbidden} {updates}")
else:
    ok("no scenario ever updates, re-enables or deletes a firewall rule")

# --- executor pool ----------------------------------------------------------------------------

POOL_ENV = {"MEERKAT_BUILDBUDDY_EXECUTOR_IMAGE": EXECUTOR_IMAGE, "MEERKAT_BUILDBUDDY_CI_IMAGE": CI_IMAGE}
SECRETS_OK = {"meerkat-bb-executor-api-key": ["executor-key"], "meerkat-bb-control-endpoint": ["grpcs://10.132.0.7:1986"],
              "meerkat-bb-control-ca": [CA_PEM]}


def secret_writes(calls):
    return [(call["words"][3], call.get("stdin")) for call in calls if "stdin" in call]


proc, calls = run(POOL, ["sync-secrets"], {"secrets": {"buildbuddy-api-key": ["cloud-key-value"]}},
                  BUILDBUDDY_GRPC_URL="grpcs://10.20.30.40:1986", BUILDBUDDY_TLS_CA_B64=CA_B64,
                  BUILDBUDDY_API_KEY="cloud-key-value")
writes = secret_writes(calls)
if (proc.returncode != 0 or writes != [("meerkat-bb-control-endpoint", "grpcs://10.20.30.40:1986"),
                                        ("meerkat-bb-control-ca", CA_PEM.rstrip("\n"))]
        or any("api-key" in " ".join(c["argv"]) for c in calls)):
    fail(f"sync-secrets wrote {writes}: {proc.stderr[-300:]}")
else:
    ok("sync-secrets writes the gRPCS endpoint and the CA, never an API key")
for label, url, ca, message in (
    ("a plaintext endpoint", "grpc://10.20.30.40:1985", CA_B64, "must be grpcs://<private IPv4>"),
    ("a public endpoint", "grpcs://203.0.113.9:1986", CA_B64, "is not a private IPv4 address"),
    ("a hostname endpoint", "grpcs://bb.example.invalid:1986", CA_B64, "must be grpcs://<private IPv4>"),
    ("a missing CA", "grpcs://10.20.30.40:1986", "", "BUILDBUDDY_TLS_CA_B64"),
):
    proc, calls = run(POOL, ["sync-secrets"], {"secrets": {}}, BUILDBUDDY_GRPC_URL=url, BUILDBUDDY_TLS_CA_B64=ca)
    if proc.returncode == 0 or secret_writes(calls) or message not in proc.stderr:
        fail(f"sync-secrets did not refuse {label} as such: {proc.stderr[-200:]}")
    else:
        ok(f"sync-secrets refuses {label}")

for label, env, message in (
    ("an executor image without a digest",
     dict(POOL_ENV, MEERKAT_BUILDBUDDY_EXECUTOR_IMAGE="buildbuddy.bbcr.io/public/buildbuddy-executor-enterprise:enterprise-v2.265.0"),
     "MEERKAT_BUILDBUDDY_EXECUTOR_IMAGE must pin the executor image by digest"),
    ("an action image without a digest",
     dict(POOL_ENV, MEERKAT_BUILDBUDDY_CI_IMAGE="europe-west1-docker.pkg.dev/proj-test/meerkat-ci/meerkat-ci-rust:1.94.0"),
     "MEERKAT_BUILDBUDDY_CI_IMAGE must pin the action image by digest"),
):
    proc, calls = run(POOL, ["ensure"], {"secrets": SECRETS_OK}, **env)
    if proc.returncode == 0 or mutating(calls) or message not in proc.stderr:
        fail(f"executor pool ensure did not refuse {label} as such: {proc.stderr[-200:]}")
    else:
        ok(f"executor pool ensure refuses {label}")

unpinned = "registry.example.invalid/meerkat-ci:1"
proc, calls = run(POOL, ["ensure"], {"secrets": SECRETS_OK}, **dict(POOL_ENV, MEERKAT_BUILDBUDDY_CI_IMAGE=unpinned))
hint = proc.stderr.replace(unpinned, "")
if proc.returncode == 0 or "scripts/gcp-buildbuddy-ci-image image" not in hint or "docker.pkg.dev/" in hint:
    fail(f"the unpinned action image error must point at the image script and name no registry path: {proc.stderr[-300:]}")
else:
    ok("the unpinned action image error points at the image script and names no registry path")

policy = {"bindings": [{"role": "roles/secretmanager.secretAccessor", "members": [f"serviceAccount:{EXECUTOR_SA}"]}]}
proc, calls = run(POOL, ["ensure"], {"secrets": SECRETS_OK, "project_iam_policy": policy}, MEERKAT_GCP_BUILDBUDDY_MANAGE_IAM="1", **POOL_ENV)
problems = []
granted = sorted(c["words"][2] for c in find(calls, "secrets", "add-iam-policy-binding")
                 if flag(c, "role") == "roles/secretmanager.secretAccessor" and flag(c, "member") == f"serviceAccount:{EXECUTOR_SA}")
if granted != sorted(SECRETS_OK):
    problems.append(f"per-secret accessor on {granted}")
if any(flag(c, "role") == "roles/secretmanager.secretAccessor" for c in find(calls, "projects", "add-iam-policy-binding")):
    problems.append("a project-wide secretAccessor binding is added")
removed = [c for c in find(calls, "projects", "remove-iam-policy-binding") if flag(c, "role") == "roles/secretmanager.secretAccessor"]
if len(removed) != 1 or "--all" not in removed[0]["argv"]:
    problems.append("the project-wide secretAccessor binding is not removed")
templates = find(calls, "compute", "instance-templates", "create")
template_name = templates[0]["words"][3] if templates else ""
script = (templates[0].get("files") or {}).get("startup-script") or "" if templates else ""
if not re.fullmatch(r"meerkat-bb-exec-template-[0-9a-f]{12}", template_name):
    problems.append(f"template name {template_name!r} is not derived from its rendering")
for needle in ('API_KEY_SECRET="meerkat-bb-executor-api-key"', 'ENDPOINT_SECRET="meerkat-bb-control-endpoint"',
               'CA_SECRET="meerkat-bb-control-ca"', f'EXECUTOR_IMAGE="{EXECUTOR_IMAGE}"', f'CI_IMAGE="{CI_IMAGE}"',
               'default_isolation_type: "oci"', 'default_image: "${CI_IMAGE}"',
               'case "${app_target}" in\n  grpcs://*) ;;\n  *)\n    echo "Refusing a non-TLS BuildBuddy app target',
               "SSL_CERT_FILE=/etc/buildbuddy/ca-bundle.pem", "umask 077",
               "Refusing a BuildBuddy app target outside the private network"):
    if needle not in script:
        problems.append(f"executor startup script lacks {needle!r}")
migs = find(calls, "compute", "instance-groups", "managed", "create")
if len(migs) != 1 or flag(migs[0], "template") != template_name:
    problems.append("the MIG is not created from the derived template")
# Run the executor's own app-target guard: gRPCS to a private IPv4 only.
guard_lines = script.splitlines()
try:
    first = guard_lines.index('case "${app_target}" in')
    host = next(i for i in range(first, len(guard_lines)) if guard_lines[i].startswith('app_host='))
    last = next(i for i in range(host, len(guard_lines)) if guard_lines[i] == "fi")
    guard = "\n".join(guard_lines[first:last + 1])
except (ValueError, StopIteration):
    guard = None
if guard is None:
    problems.append("executor startup script has no app-target guard")
else:
    for target, accepted in (("grpcs://10.132.0.7:1986", True), ("grpcs://203.0.113.9:1986", False),
                             ("grpc://10.132.0.7:1985", False), ("grpcs://bb.example.invalid:1986", False)):
        verdict = subprocess.run(["bash", "-c", f"set -euo pipefail\napp_target='{target}'\n{guard}\n"],
                                 capture_output=True, text=True).returncode == 0
        if verdict != accepted:
            problems.append(f"executor app-target guard {'refuses' if accepted else 'accepts'} {target}")
if proc.returncode != 0 or problems:
    fail(f"executor pool ensure: exit {proc.returncode}: {problems}: {proc.stderr[-300:]}")
else:
    ok("executor pool ensure grants Secret Manager access per secret, drops the project-wide grant, and derives the template")

def template_body(script_text, account=EXECUTOR_SA, network="default", tags=None,
                  image="projects/debian-cloud/global/images/family/debian-12", extra_metadata=None):
    items = [{"key": "startup-script", "value": script_text}] + (extra_metadata or [])
    body = {
        "metadata": {"items": items},
        "serviceAccounts": [{"email": account, "scopes": ["https://www.googleapis.com/auth/cloud-platform"]}],
        "networkInterfaces": [{"network": f"https://www.googleapis.com/compute/v1/projects/{PROJECT}/global/networks/{network}"}],
        "machineType": "c3d-standard-30",
        "disks": [{"boot": True, "initializeParams": {"sourceImage": image}},
                  {"boot": False, "deviceName": "bb-exec-cache"}],
    }
    if tags:
        body["tags"] = {"items": tags}
    return {"name": template_name, "properties": body,
            "selfLink": f"https://compute/projects/{PROJECT}/global/instanceTemplates/{template_name}"}


template_ok = template_body(script)
for label, body, message, extra_env in (
    ("a different startup script", template_body("#!/bin/bash\necho other\n"), "startup script digest", {}),
    ("the wrong service account", template_body(script, account=f"other@{PROJECT}.iam.gserviceaccount.com"),
     "service accounts", {}),
    ("network tags", template_body(script, tags=["http-server"]), "network tags", {}),
    ("extra metadata", template_body(script, extra_metadata=[{"key": "ssh-keys", "value": "x"}]), "metadata keys", {}),
    ("another boot image", template_body(script, image="projects/other/global/images/family/custom"), "boot image", {}),
):
    proc, calls = run(POOL, ["ensure"], {"secrets": SECRETS_OK, "instance_templates": {template_name: body}},
                      **POOL_ENV, **extra_env)
    if proc.returncode == 0 or message not in proc.stderr or mutating(calls):
        fail(f"executor pool ensure reused a template with {label}: {proc.stderr[-300:]}")
    else:
        ok(f"executor pool ensure refuses a template of the derived name with {label}")
override = dict(template_body(script, account=f"other@{PROJECT}.iam.gserviceaccount.com"), name="custom-template")
proc, calls = run(POOL, ["ensure"], {"secrets": SECRETS_OK, "instance_templates": {"custom-template": override}},
                  MEERKAT_GCP_BUILDBUDDY_TEMPLATE="custom-template", **POOL_ENV)
if proc.returncode == 0 or "service accounts" not in proc.stderr or mutating(calls):
    fail(f"executor pool ensure reused an explicitly named template with the wrong service account: {proc.stderr[-300:]}")
else:
    ok("executor pool ensure refuses an explicitly named template with the wrong service account")
proc, calls = run(POOL, ["ensure"], {"secrets": SECRETS_OK, "instance_templates": {template_name: template_ok}}, **POOL_ENV)
if proc.returncode != 0 or find(calls, "compute", "instance-templates", "create"):
    fail(f"executor pool ensure did not reuse a matching template: {proc.stderr[-300:]}")
else:
    ok("executor pool ensure reuses a template whose security-relevant properties match")

mig = {"name": "meerkat-bb-exec", "instanceTemplate": template_ok["selfLink"]}
stale = [{"instance": "https://compute/zones/z/instances/meerkat-bb-exec-abcd", "instanceStatus": "TERMINATED",
          "version": {"instanceTemplate": "https://compute/global/instanceTemplates/meerkat-bb-exec-template-v4"}}]
proc, calls = run(POOL, ["up"], {"secrets": SECRETS_OK, "instance_templates": {template_name: template_ok},
                                 "migs": {"meerkat-bb-exec": mig}, "mig_instances": {"meerkat-bb-exec": stale}}, **POOL_ENV)
resized = [c for c in find(calls, "compute", "instance-groups", "managed", "update") if flag_values(c, "size")]
if proc.returncode == 0 or "meerkat-bb-exec-template-v4" not in proc.stderr or resized:
    fail(f"up started executors made from another template: {proc.stderr[-300:]}")
else:
    ok("up refuses while pool instances come from another template")

for label, env in (
    ("the endpoint secret aliasing the executor key", {"MEERKAT_BUILDBUDDY_ENDPOINT_SECRET": "meerkat-bb-executor-api-key"}),
    ("the CA secret aliasing the executor key", {"MEERKAT_BUILDBUDDY_CA_SECRET": "meerkat-bb-executor-api-key"}),
    ("the CA secret aliasing the endpoint secret", {"MEERKAT_BUILDBUDDY_CA_SECRET": "meerkat-bb-control-endpoint"}),
):
    for command, extra in ((["sync-secrets"], {"BUILDBUDDY_GRPC_URL": "grpcs://10.20.30.40:1986", "BUILDBUDDY_TLS_CA_B64": CA_B64}),
                           (["ensure"], dict(POOL_ENV, MEERKAT_GCP_BUILDBUDDY_MANAGE_IAM="1"))):
        proc, calls = run(POOL, command, {"secrets": SECRETS_OK}, **extra, **env)
        if proc.returncode == 0 or "must be three different secrets" not in proc.stderr or mutating(calls):
            fail(f"{command[0]} accepted {label}: {mutating(calls)} {proc.stderr[-200:]}")
        else:
            ok(f"{command[0]} refuses {label} before any write")

proc, calls = run(POOL, ["ensure"], {"secrets": dict(SECRETS_OK, **{"meerkat-bb-executor-api-key": []})}, **POOL_ENV)
if (proc.returncode == 0 or find(calls, "compute", "instance-templates", "create")
        or "meerkat-bb-executor-api-key has no version" not in proc.stderr):
    fail("executor pool ensure ran without an executor key")
else:
    ok("executor pool ensure refuses an executor key secret without a version")

# --- the GCP project is required ---------------------------------------------------------

for script, label in ((CONTROL, "control plane"), (POOL, "executor pool")):
    proc, calls = run(script, ["status"], {}, MEERKAT_GCP_PROJECT_ID="")
    if proc.returncode == 0 or calls or "MEERKAT_GCP_PROJECT_ID must name the GCP project" not in proc.stderr:
        fail(f"{label} ran without a GCP project: rc={proc.returncode} {[c['argv'] for c in calls]} {proc.stderr[-200:]}")
    else:
        ok(f"{label} refuses to run without MEERKAT_GCP_PROJECT_ID and calls nothing")
    proc, calls = run(script, ["status"], {}, MEERKAT_GCP_PROJECT_ID="", GCP_PROJECT_ID=PROJECT)
    projects = {value for call in calls for value in flag_values(call, "project")}
    if proc.returncode != 0 or projects != {PROJECT}:
        fail(f"{label} does not fall back to GCP_PROJECT_ID: rc={proc.returncode} {projects} {proc.stderr[-200:]}")
    else:
        ok(f"{label} reads GCP_PROJECT_ID when MEERKAT_GCP_PROJECT_ID is unset")

# --- the CI image script needs the project wherever it derives an image ------------------

CI_IMAGE_SCRIPT = os.path.join(REPO, "scripts", "gcp-buildbuddy-ci-image")
refused_context = os.path.join(ROOT, "ci-image-context-refused")
for label, args in (("image", ["image"]), ("build", ["build"]), ("context", ["context", refused_context])):
    proc, calls = run(CI_IMAGE_SCRIPT, args, {}, cwd=REPO, MEERKAT_GCP_PROJECT_ID="")
    if proc.returncode != 2 or calls or "MEERKAT_GCP_PROJECT_ID must name the GCP project" not in proc.stderr:
        fail(f"ci-image {label} ran without a GCP project: rc={proc.returncode} {[c['argv'] for c in calls]} {proc.stderr[-200:]}")
    else:
        ok(f"ci-image {label} refuses to run without MEERKAT_GCP_PROJECT_ID and calls nothing")
if os.path.exists(refused_context):
    fail("ci-image context wrote a build context without a GCP project")
base_context = os.path.join(ROOT, "ci-image-context-base")
base_image = "registry.example.invalid/meerkat-ci-base:1"
proc, calls = run(CI_IMAGE_SCRIPT, ["context", base_context], {}, cwd=REPO, MEERKAT_GCP_PROJECT_ID="",
                  MEERKAT_CI_BASE_IMAGE=base_image)
dockerfile = os.path.join(base_context, "Dockerfile")
first_line = open(dockerfile, encoding="utf-8").readline().strip() if os.path.exists(dockerfile) else ""
if proc.returncode != 0 or calls or first_line != f"FROM {base_image}":
    fail(f"ci-image context with MEERKAT_CI_BASE_IMAGE: rc={proc.returncode} {first_line!r} {proc.stderr[-200:]}")
else:
    ok("ci-image context builds FROM MEERKAT_CI_BASE_IMAGE without a project")
pinned_image = "registry.example.invalid/meerkat-ci:1"
proc, calls = run(CI_IMAGE_SCRIPT, ["image"], {}, cwd=REPO, MEERKAT_GCP_PROJECT_ID="", MEERKAT_CI_IMAGE=pinned_image)
if proc.returncode != 0 or calls or proc.stdout.strip() != pinned_image:
    fail(f"ci-image image with MEERKAT_CI_IMAGE: rc={proc.returncode} {proc.stdout.strip()!r} {proc.stderr[-200:]}")
else:
    ok("ci-image image prints MEERKAT_CI_IMAGE without a project")
proc, calls = run(CI_IMAGE_SCRIPT, ["--help"], {}, cwd=REPO, MEERKAT_GCP_PROJECT_ID="")
if proc.returncode != 0 or calls or not proc.stdout.startswith("usage: gcp-buildbuddy-ci-image"):
    fail(f"ci-image --help without a project: rc={proc.returncode} {proc.stderr[-200:]}")
else:
    ok("ci-image --help needs no project")
proc, calls = run(CI_IMAGE_SCRIPT, ["image"], {}, cwd=REPO)
if proc.returncode != 0 or calls or f"-docker.pkg.dev/{PROJECT}/meerkat-ci/meerkat-ci-rust:" not in proc.stdout:
    fail(f"ci-image image does not derive its reference from MEERKAT_GCP_PROJECT_ID: {proc.stdout.strip()!r}")
else:
    ok("ci-image image derives its reference from MEERKAT_GCP_PROJECT_ID")

# --- the self-hosted RBE platform names no image ---------------------------------------------

platforms_build = open(os.path.join(REPO, "tools", "bazel", "platforms", "BUILD.bazel"), encoding="utf-8").read()
gcp_block = re.search(r'platform\(\n    name = "linux_x86_64_gcp_ci",\n(.*?)\n\)\n', platforms_build, re.S)
gcp_images = re.findall(r'"container-image":\s*"([^"]*)"', gcp_block.group(1)) if gcp_block else None
if gcp_images != [""]:
    fail(f"linux_x86_64_gcp_ci must unset container-image with an empty string, not name an image: {gcp_images}")
else:
    ok("linux_x86_64_gcp_ci unsets the container-image it would inherit")
registry_path = re.compile(r"[a-z0-9-]+-docker\.pkg\.dev/[a-z][a-z0-9-]{4,28}[a-z0-9]/")
scanned = [os.path.join(REPO, ".bazelrc")]
for directory, _, names in os.walk(os.path.join(REPO, "tools", "bazel")):
    scanned += [os.path.join(directory, name) for name in names
                if name in ("BUILD", "BUILD.bazel") or name.endswith(".bzl")]
scripts_dir = os.path.join(REPO, "scripts")
scanned += [os.path.join(scripts_dir, name) for name in sorted(os.listdir(scripts_dir))
            if name.startswith(("ci-action-pins", "gcp-buildbuddy-"))]
written = []
for path in scanned:
    with open(path, encoding="utf-8") as handle:
        written += [f"{os.path.relpath(path, REPO)}:{number}" for number, line in enumerate(handle, 1)
                    if registry_path.search(line)]
if written:
    fail(f"a project-scoped registry path is written out: {written}")
else:
    ok("no project-scoped registry path is written out in the Bazel platforms, the pin check or the GCP scripts")

sys.exit(1 if FAILURES else 0)
PY

set +e
python3 "${TEST_ROOT}/scenarios.py" "${REPO_ROOT}" "${FAKE_BIN}" "${TEST_ROOT}"
scenario_status=$?
set -e
if [[ "${scenario_status}" -ne 0 ]]; then
  fail "control plane / executor pool scenarios (see FAIL lines above)"
fi

# --- workflow wiring ------------------------------------------------------------------

workflow="${REPO_ROOT}/.github/workflows/buildbuddy.yml"
batch_action="${REPO_ROOT}/.github/actions/run-buildbuddy-ci-lane-batch/action.yml"
if grep -q 'secrets\.BUILDBUDDY_API_KEY' "${workflow}"; then
  fail "buildbuddy.yml reads the BuildBuddy Cloud key (secrets.BUILDBUDDY_API_KEY)"
elif ! grep -qF 'BUILDBUDDY_API_KEY: ${{ secrets.MEERKAT_GCP_BUILDBUDDY_API_KEY }}' "${workflow}"; then
  fail "buildbuddy.yml does not take its API key from secrets.MEERKAT_GCP_BUILDBUDDY_API_KEY"
else
  pass "buildbuddy.yml uses the self-hosted key, never the BuildBuddy Cloud key"
fi
missing_wiring=()
for needle in \
  'run: scripts/test-gcp-buildbuddy-hardening.sh' \
  'run: scripts/gcp-buildbuddy-control-plane ensure' \
  'run: scripts/gcp-buildbuddy-control-plane verify --wait' \
  'executor_grpc_url: ${{ steps.endpoint.outputs.executor_grpc_url }}' \
  'tls_ca_b64: ${{ steps.endpoint.outputs.tls_ca_b64 }}' \
  'BUILDBUDDY_GRPC_URL: ${{ needs.control-plane-up.outputs.executor_grpc_url }}' \
  '  control-plane-down:' \
  'run: scripts/gcp-buildbuddy-control-plane stop' \
  '      - control-plane-down'; do
  grep -qF -- "${needle}" "${workflow}" || missing_wiring+=("${needle}")
done
grep -qF 'run: scripts/gcp-buildbuddy-control-plane tunnel' "${batch_action}" || missing_wiring+=("the batch action opens the IAP tunnel")
if [[ "$(grep -c 'BUILDBUDDY_TLS_CA_B64: ${{ needs.control-plane-up.outputs.tls_ca_b64 }}' "${workflow}")" -ne 11 ]]; then
  missing_wiring+=("the CA reaches executors-up and all ten submitters")
fi
if grep -q '0\.0\.0\.0/0' "${workflow}" "${batch_action}"; then
  missing_wiring+=("no 0.0.0.0/0 in the workflow")
fi
if [[ "${#missing_wiring[@]}" -gt 0 ]]; then
  fail "buildbuddy.yml wiring lacks: ${missing_wiring[*]}"
else
  pass "buildbuddy.yml verifies the installed config, tunnels submitters, passes the CA and stops the control plane after use"
fi

if [[ "${failures}" -gt 0 ]]; then
  echo "${failures} failure(s)" >&2
  exit 1
fi
echo "PASS: self-hosted GCP BuildBuddy hardening contract"
