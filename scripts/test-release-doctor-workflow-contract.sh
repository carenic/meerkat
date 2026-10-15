#!/usr/bin/env bash
# Contract test for the release doctor's release-workflow assertions.
#
# Reproduces the #1091 drift shape: `.github/workflows/release.yml` was
# reworded after v0.8.32 (a folded `if: >-` condition and a `--slo-seconds
# ${{ ... }}` expression) without changing behaviour, and two doctor greps
# that matched literal lines failed on a healthy main. The fix asserts what
# the workflow does under concrete events. This test proves both halves of
# that contract on fixtures derived from the committed workflow: equivalent
# spellings pass, and removing the behaviour still fails and names the defect.
#
# Every fixture mutation asserts its needle occurs exactly once in the real
# workflow, so a rewording that invalidates a fixture fails here loudly
# instead of leaving the doctor unverified.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PYTHON="$("$(dirname "${BASH_SOURCE[0]}")/require-python" 3.11 "$(basename "${BASH_SOURCE[0]}")")" || exit 1
CHECKER="${REPO_ROOT}/scripts/check_release_workflow_contract.py"
DOCTOR="${REPO_ROOT}/scripts/release-doctor"
WORKFLOW="${REPO_ROOT}/.github/workflows/release.yml"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/meerkat-release-doctor-contract.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

SEMVER_PASS="PASS Tag releases reuse exact-tree pre-tag semver evidence"
SEMVER_FAIL="FAIL Tag releases must reuse exact-tree pre-tag semver evidence"
SLO_PASS="PASS Rust registry publication reads crates back before the SDKs and enforces the 30 minute publication SLO"
SLO_FAIL="FAIL Rust registry publication does not read crates back before the SDKs or enforce the 30 minute publication SLO"
DISPATCH_PASS="PASS Publishing dispatches are bound to their version's tag"
DISPATCH_FAIL="FAIL A publishing dispatch without release_tag could publish a branch tip as the tagged version"
TAG_VALIDATION_PASS="PASS Tag releases validate on the verified schema-4 exact-tree CI attestation instead of re-running Cargo validation"
TAG_VALIDATION_FAIL="FAIL Tag releases must validate on the verified schema-4 exact-tree CI attestation"
PREBUILD_PASS="PASS Pre-tag prebuilds publish nothing, and a tag publishes its own commit's prebuild only through the attested-archive verification"
PREBUILD_FAIL="FAIL A pre-tag prebuild must publish nothing, and a tag may publish its commit's prebuild only through the attested-archive verification"
BASELINE_PASS="PASS Semver baseline rustdoc builds alongside the release and gates nothing; it attaches only to a published release"
BASELINE_FAIL="FAIL The semver baseline rustdoc must stay off every path a user waits on"
TRAILING_PASS="PASS Linux and macOS arm64 assets publish without waiting for the trailing Intel macOS and Windows lanes; no upload replaces a published asset"
TRAILING_FAIL="FAIL The trailing Intel macOS and Windows lanes must not gate the critical assets, and no upload may replace a published asset"

fail() {
  echo "release doctor workflow contract violated: $1" >&2
  shift
  for extra in "$@"; do
    echo "  ${extra}" >&2
  done
  exit 1
}

# mutate <name> <destination>: write a mutated copy of the committed workflow.
mutate() {
  local name="$1"
  local destination="$2"
  "$PYTHON" - "$WORKFLOW" "$destination" "$name" <<'PYEOF'
import pathlib
import re
import sys

source, destination, name = sys.argv[1:4]
text = pathlib.Path(source).read_text()


def replace_once(needle: str, replacement: str) -> None:
    global text
    count = text.count(needle)
    if count != 1:
        raise SystemExit(
            f"fixture `{name}` expects exactly one occurrence of {needle!r} "
            f"in the committed workflow, found {count}; update the fixture"
        )
    text = text.replace(needle, replacement)


EVIDENCE_STEP = re.compile(
    r"      - name: Verify exact-tree pre-tag semver evidence\n"
    r"(?:        .*\n|          .*\n|\n)*?"
    r"(?=      - name: )"
)
EVIDENCE_IF = (
    "            github.event_name != 'workflow_dispatch' ||\n"
    "            (github.event.inputs.release_tag != '' &&\n"
    "             github.event.inputs.semver_evidence_job_id == '')\n"
)
MEASUREMENT_STEP_IF = (
    "      - name: Verify every reported break is named and the notes are stamped\n"
    "        if: >-\n"
    "          ${{\n"
    "            github.event_name == 'workflow_dispatch' &&\n"
    "            github.event.inputs.semver_evidence_job_id == '' &&\n"
    "            github.event.inputs.release_tag == ''\n"
    "          }}\n"
)
SLO_LINE = (
    "            --slo-seconds ${{ github.event_name == 'workflow_dispatch'"
    " && '2147483647' || '1800' }}\n"
)
SLO_PREVIOUS_LINE = (
    '            ${tag_pushed_at:+--tag-pushed-at "${tag_pushed_at}"} \\\n'
)

STEP_BODY = r"(?:        .*\n|          .*\n|\n)*?(?=      - name: |\Z|  [a-z_]+:\n)"
READBACK_STEP = re.compile(r"      - name: Verify all Rust crates are public\n" + STEP_BODY)
SLO_STEP = re.compile(r"      - name: Enforce the 30 minute Rust publication SLO\n" + STEP_BODY)
PYTHON_SDK_STEP = re.compile(r"      - name: Publish Python SDK\n")

if name == "evidence-step-removed":
    match = EVIDENCE_STEP.search(text)
    if match is None or len(EVIDENCE_STEP.findall(text)) != 1:
        raise SystemExit("fixture expects exactly one exact-tree evidence step")
    text = text[: match.start()] + text[match.end() :]
elif name == "evidence-step-off-tags":
    replace_once(
        EVIDENCE_IF,
        "            github.event_name == 'workflow_dispatch' &&\n"
        "            (github.event.inputs.release_tag != '' &&\n"
        "             github.event.inputs.semver_evidence_job_id == '')\n",
    )
elif name == "evidence-if-single-line":
    # Same condition, collapsed onto one line with a bare (unwrapped)
    # expression: the spelling GitHub also accepts.
    condition = " ".join(EVIDENCE_IF.split())
    replace_once(
        "        if: >-\n          ${{\n" + EVIDENCE_IF + "          }}\n        shell: bash\n"
        "        env:\n          GH_TOKEN: ${{ github.token }}\n        run: |\n"
        "          set -euo pipefail\n          tree_sha=",
        f"        if: {condition}\n        shell: bash\n"
        "        env:\n          GH_TOKEN: ${{ github.token }}\n        run: |\n"
        "          set -euo pipefail\n          tree_sha=",
    )
elif name == "measurement-on-tags":
    replace_once(
        MEASUREMENT_STEP_IF,
        "      - name: Verify every reported break is named and the notes are stamped\n",
    )
elif name == "slo-relaxed":
    replace_once(SLO_LINE, SLO_LINE.replace("'1800'", "'3600'"))
elif name == "slo-flag-removed":
    replace_once(
        SLO_PREVIOUS_LINE + SLO_LINE,
        '            ${tag_pushed_at:+--tag-pushed-at "${tag_pushed_at}"}\n',
    )
elif name == "slo-literal":
    replace_once(SLO_LINE, "            --slo-seconds 1800\n")
elif name == "slo-reflowed":
    # Equivalent expression, operands reordered and split across lines.
    replace_once(
        SLO_LINE,
        "            --slo-seconds ${{\n"
        "              github.event_name != 'workflow_dispatch' && '1800'\n"
        "              || '2147483647'\n"
        "            }}\n",
    )
elif name == "readback-step-removed":
    match = READBACK_STEP.search(text)
    if match is None or len(READBACK_STEP.findall(text)) != 1:
        raise SystemExit("fixture expects exactly one crate readback step")
    text = text[: match.start()] + text[match.end() :]
elif name == "slo-before-sdk":
    slo = SLO_STEP.search(text)
    sdk = PYTHON_SDK_STEP.search(text)
    if slo is None or sdk is None:
        raise SystemExit("fixture expects the SLO step and the Python SDK step")
    slo_text = slo.group(0)
    text = text[: slo.start()] + text[slo.end() :]
    sdk = PYTHON_SDK_STEP.search(text)
    text = text[: sdk.start()] + slo_text + text[sdk.start() :]
elif name == "dispatch-unbound":
    # The pre-fix condition: a publishing dispatch with no release_tag
    # skipped the version check and published the branch tip.
    replace_once(
        "          (github.event_name == 'workflow_dispatch' &&\n"
        "           (github.event.inputs.release_tag != '' ||\n"
        "            github.event.inputs.publish_release_packages == 'true' ||\n"
        "            github.event.inputs.publish_web_sdk_only == 'true' ||\n"
        "            github.event.inputs.publish_release_assets_only == 'true'))\n",
        "          (github.event_name == 'workflow_dispatch' &&\n"
        "           github.event.inputs.release_tag != '')\n",
    )
elif name == "dispatch-branch-name":
    # The pre-round-2 comparison: the last path segment of the ref name,
    # so a branch named release/v<version> passed and could publish.
    binding = re.compile(
        r'          expected="v\$\{version\}"\n(?:.*\n)*?'
        r'          echo "Version \$\{version\} matches release tag \$\{release_tag\}"\n'
    )
    text, count = binding.subn(
        '          ref_name="${RELEASE_TAG_INPUT:-${GITHUB_REF_NAME}}"\n'
        '          tag="${ref_name##*/}"\n'
        '          tag="${tag#v}"\n'
        '          if [[ "${version}" != "${tag}" ]]; then\n'
        '            exit 1\n'
        '          fi\n',
        text,
    )
    if count != 1:
        raise SystemExit(f"fixture `{name}` expects one release-version script, found {count}")
elif name == "dispatch-head-unchecked":
    # Drop the guard that the named release_tag is the checked-out commit.
    replace_once(
        '            if [[ "$(git rev-parse HEAD)" != "${tag_commit}" ]]; then\n'
        '              echo "The checked-out commit is not the ${RELEASE_TAG_INPUT} tag." >&2\n'
        "              exit 1\n"
        "            fi\n",
        "",
    )
elif name == "dispatch-alpha-unscoped":
    # Allow the alpha tag namespace outside the alpha crates-only lane.
    replace_once(
        '          if [[ "${ALPHA_CRATES_ONLY}" == "true" ]]; then\n'
        '            allowed+=("alpha/${expected}")\n'
        "          fi\n",
        '          allowed+=("alpha/${expected}")\n',
    )
elif name == "dispatch-checkout-github-ref":
    # require_ci_green checks out github.ref instead of the named release_tag.
    job = text.index("  require_ci_green:\n")
    needle = (
        "          ref: ${{ github.event_name == 'workflow_dispatch' && "
        "github.event.inputs.release_tag != '' && github.event.inputs.release_tag || github.ref }}\n"
    )
    at = text.index(needle, job)
    text = text[:at] + "          ref: ${{ github.ref }}\n" + text[at + len(needle):]
elif name == "dispatch-prefers-github-ref":
    # Prefer a tag github.ref over the named release_tag: a dispatch on the
    # version's tag that names a branch checks out and publishes the branch.
    replace_once(
        '          if [[ -n "${RELEASE_TAG_INPUT}" ]]; then\n',
        '          if [[ "${GITHUB_REF}" == refs/tags/* ]]; then\n'
        '            release_tag="${GITHUB_REF#refs/tags/}"\n'
        '          elif [[ -n "${RELEASE_TAG_INPUT}" ]]; then\n',
    )
elif name == "dispatch-alpha-any-version":
    # Accept any alpha/* tag in the alpha lane, not only this version's.
    replace_once(
        '            if [[ "${release_tag}" == "${tag}" ]]; then\n',
        '            if [[ "${release_tag}" == "${tag}" || ( "${ALPHA_CRATES_ONLY}" == "true" && "${release_tag}" == alpha/* ) ]]; then\n',
    )
elif name == "dispatch-ref-name":
    # Equivalent: read a tag push's tag from GITHUB_REF_NAME, which GitHub
    # sets to the ref without refs/tags/ (alpha/v<version> stays whole).
    replace_once(
        '            release_tag="${GITHUB_REF#refs/tags/}"\n',
        '            release_tag="${GITHUB_REF_NAME}"\n',
    )
elif name == "dispatch-branch-ref-name":
    # Read a non-tag ref's name without requiring refs/tags/: a branch named
    # exactly like an allowed tag (refs/heads/v<version>) then binds.
    replace_once(
        '          elif [[ "${GITHUB_REF}" == refs/tags/* ]]; then\n'
        '            release_tag="${GITHUB_REF#refs/tags/}"\n'
        "          else\n"
        '            echo "${GITHUB_REF} is not a tag. A publishing run must run on an allowed tag (${allowed[*]}) or name release_tag." >&2\n'
        "            exit 1\n"
        "          fi\n",
        "          else\n"
        '            release_tag="${GITHUB_REF_NAME}"\n'
        "          fi\n",
    )
elif name == "dispatch-branch-ref-name-tag-exists":
    # Read a non-tag ref's name as the tag once a tag of that name exists: a
    # branch named like the version's tag then publishes its tip as that tag.
    replace_once(
        '          elif [[ "${GITHUB_REF}" == refs/tags/* ]]; then\n'
        '            release_tag="${GITHUB_REF#refs/tags/}"\n'
        "          else\n"
        '            echo "${GITHUB_REF} is not a tag. A publishing run must run on an allowed tag (${allowed[*]}) or name release_tag." >&2\n'
        "            exit 1\n"
        "          fi\n",
        "          else\n"
        '            release_tag="${GITHUB_REF_NAME}"\n'
        '            if ! git rev-parse -q --verify "refs/tags/${release_tag}^{commit}" >/dev/null; then\n'
        "              exit 1\n"
        "            fi\n"
        "          fi\n",
    )
elif name == "dispatch-ref-name-expression":
    # Equivalent: a tag push's tag read from the ${{ github.ref_name }}
    # expression (the spelling release.yml uses elsewhere).
    replace_once(
        '            release_tag="${GITHUB_REF#refs/tags/}"\n',
        '            release_tag="${{ github.ref_name }}"\n',
    )
elif name == "dispatch-prefers-github-ref-other-tag":
    # Prefer a tag github.ref over the named release_tag, with no check of
    # the named tag at all.
    binding = re.compile(
        r'          if \[\[ -n "\$\{RELEASE_TAG_INPUT\}" \]\]; then\n(?:.*\n)*?'
        r'          fi\n(?=          bound=false\n)'
    )
    text, count = binding.subn(
        '          if [[ "${GITHUB_REF}" == refs/tags/* ]]; then\n'
        '            release_tag="${GITHUB_REF#refs/tags/}"\n'
        "          else\n"
        '            release_tag="${RELEASE_TAG_INPUT}"\n'
        "          fi\n",
        text,
    )
    if count != 1:
        raise SystemExit(f"fixture `{name}` expects one release-tag selection, found {count}")
elif name == "dispatch-prefers-github-ref-head-checked":
    # Prefer a tag github.ref, and check that it is the checked-out commit:
    # a named alpha tag on the same commit then publishes outside its lane.
    replace_once(
        '          if [[ -n "${RELEASE_TAG_INPUT}" ]]; then\n',
        '          if [[ "${GITHUB_REF}" == refs/tags/* ]]; then\n'
        '            release_tag="${GITHUB_REF#refs/tags/}"\n'
        '            if [[ "$(git rev-parse HEAD)" != "$(git rev-parse -q --verify "refs/tags/${release_tag}^{commit}")" ]]; then\n'
        "              exit 1\n"
        "            fi\n"
        '          elif [[ -n "${RELEASE_TAG_INPUT}" ]]; then\n',
    )
elif name == "dispatch-prefix-glob":
    # Compare the tag by prefix: v<version>1 and alpha/v<version>-rc.1 bind.
    replace_once(
        '            if [[ "${release_tag}" == "${tag}" ]]; then\n',
        '            if [[ "${release_tag}" == "${tag}"* ]]; then\n',
    )
elif name == "dispatch-alpha-named-any-version":
    # Accept any alpha/* tag the alpha lane names as release_tag.
    replace_once(
        '            release_tag="${RELEASE_TAG_INPUT}"\n',
        '            release_tag="${RELEASE_TAG_INPUT}"\n'
        '            if [[ "${ALPHA_CRATES_ONLY}" == "true" && "${RELEASE_TAG_INPUT}" == alpha/* ]]; then\n'
        '              allowed+=("${RELEASE_TAG_INPUT}")\n'
        "            fi\n",
    )
elif name == "dispatch-skips-named-tag-run":
    # Skip the binding step when a dispatch on a v* tag also names a
    # release_tag, which is then checked out and published unbound.
    replace_once(
        "          startsWith(github.ref, 'refs/tags/v') ||\n"
        "          (github.event_name == 'workflow_dispatch' &&\n",
        "          (startsWith(github.ref, 'refs/tags/v') && github.event.inputs.release_tag == '') ||\n"
        "          (github.event_name == 'workflow_dispatch' && !startsWith(github.ref, 'refs/tags/v') &&\n",
    )
elif name == "dispatch-reflowed":
    # Equivalent: the same publishing modes, reordered onto one line.
    replace_once(
        "          (github.event_name == 'workflow_dispatch' &&\n"
        "           (github.event.inputs.release_tag != '' ||\n"
        "            github.event.inputs.publish_release_packages == 'true' ||\n"
        "            github.event.inputs.publish_web_sdk_only == 'true' ||\n"
        "            github.event.inputs.publish_release_assets_only == 'true'))\n",
        "          (github.event_name == 'workflow_dispatch' && (github.event.inputs.publish_release_assets_only == 'true' || github.event.inputs.publish_web_sdk_only == 'true' || github.event.inputs.publish_release_packages == 'true' || github.event.inputs.release_tag != ''))\n",
    )
elif name == "tag-validation-cargo-on-attested-tags":
    # The pre-change route: a tag re-runs Cargo validation whenever the
    # self-hosted graph is off, whatever its exact-tree CI already covered.
    replace_once(
        " &&\n       needs.require_ci_green.outputs.ci_covers_release_validation != 'true') ||\n",
        ") ||\n",
    )
elif name == "tag-validation-gate-any-schema":
    # The gate trusts any verified attestation, so one that does not prove
    # the Cargo lanes ran skips Cargo validation too.
    replace_once(
        '            if [[ "${ci_covers}" == "true" ]]; then\n',
        '            if [[ -n "${ci_attestation_schema}" ]]; then\n',
    )
elif name == "tag-validation-schema-unwritten":
    # The verifier no longer exposes whether the attestation covers the lanes.
    replace_once('          echo "covers_release_validation=${covers}" >> "${GITHUB_OUTPUT}"\n', "")
elif name == "tag-validation-ignores-sdk-host":
    # The pre-review coverage: any schema-4 attestation, SDK suites or not.
    pattern = re.compile(
        r"              \(\.component_results\.sdk_host == \"success\" or\n(?:.*\n)*?"
        r"                \(\(\.reused_test_evidence\.workflow_run_attempt // \"\"\) \| test\(\"\^\[1-9\]\[0-9\]\*\$\"\)\)\)\)' \\\n"
    )
    text, count = pattern.subn("              true' \\\n", text)
    if count != 1:
        raise SystemExit(f"fixture `{name}` expects one SDK-lane clause, found {count}")
elif name == "tag-validation-reflowed":
    # Equivalent: the tag disjunct on one line, its operands reordered.
    replace_once(
        "      (github.event_name != 'workflow_dispatch' &&\n"
        "       vars.MEERKAT_GCP_BUILDBUDDY_ENABLED != 'true' &&\n"
        "       needs.require_ci_green.outputs.ci_covers_release_validation != 'true') ||\n",
        "      (needs.require_ci_green.outputs.ci_covers_release_validation != 'true' && "
        "vars.MEERKAT_GCP_BUILDBUDDY_ENABLED != 'true' && "
        "github.event_name != 'workflow_dispatch') ||\n",
    )
elif name == "prebuild-publishes":
    # A prebuild dispatch reaches the GitHub release publisher.
    replace_once(
        "  publish_github_release:\n    name: Publish GitHub release assets\n    if: >-\n",
        "  publish_github_release:\n    name: Publish GitHub release assets\n    if: >-\n"
        "      github.event.inputs.prebuild_release_artifacts == 'true' ||\n",
    )
elif name == "prebuild-moving-checkout":
    # The Web SDK prebuild checks out main's moving tip, not the dispatched commit.
    job = text.index("  build_web_sdk_package:\n")
    needle = (
        "          ref: ${{ github.event_name == 'workflow_dispatch' && "
        "github.event.inputs.prebuild_release_artifacts == 'true' && github.sha || "
    )
    at = text.index(needle, job)
    text = text[:at] + "          ref: ${{ " + text[at + len(needle):]
elif name == "prebuild-builds-anyway":
    # A tag whose commit was prebuilt rebuilds every binary anyway.
    needle = "      needs.release_prebuild.outputs.run_id == '' &&\n"
    if text.count(needle) != 4:
        raise SystemExit(f"fixture `{name}` expects four binary build guards, found {text.count(needle)}")
    text = text.replace(needle, "")
elif name == "prebuild-reuse-unverified":
    # The publisher takes the prebuild's archives through the exact-tag
    # verification, which a main-attested archive cannot pass honestly, and
    # drops the --source-run binding.
    pattern = re.compile(
        r'          if \[\[ -n "\$\{PREBUILD_RUN_ID\}" \]\]; then\n'
        r"            # The tag commit's pre-tag prebuild(?:.*\n)*?          elif "
    )
    text, count = pattern.subn("          if ", text)
    if count != 1:
        raise SystemExit(f"fixture `{name}` expects one prebuild verification branch, found {count}")
elif name == "prebuild-web-unverified":
    match = re.search(
        r"      - name: Verify the prebuilt Web SDK package\n" + STEP_BODY, text
    )
    if match is None:
        raise SystemExit(f"fixture `{name}` expects the Web SDK verification step")
    text = text[: match.start()] + text[match.end() :]
elif name == "prebuild-gate-renamed":
    replace_once("    name: Release prebuild gate\n", "    name: Prebuild complete\n")
elif name == "prebuild-lookup-if-reflowed":
    # Equivalent: the lookup's condition folded, its operands reordered.
    replace_once(
        "    if: ${{ startsWith(github.ref, 'refs/tags/v') && github.event_name != 'workflow_dispatch' }}\n",
        "    if: >-\n      github.event_name != 'workflow_dispatch' &&\n      startsWith(github.ref, 'refs/tags/v')\n",
    )
elif name == "semver-baseline-gates-homebrew":
    # The Homebrew update waits for the 17-minute baseline rustdoc build.
    job = text.index("  update_homebrew:\n")
    needle = "    needs: [publish_github_release]\n"
    at = text.index(needle, job)
    text = text[:at] + "    needs: [publish_github_release, publish_semver_baseline]\n" + text[at + len(needle):]
elif name == "semver-baseline-after-release":
    # The pre-change shape: the rustdoc build starts only after publication.
    replace_once(
        "    name: Generate semver baseline rustdoc\n    needs: [require_ci_green]\n",
        "    name: Generate semver baseline rustdoc\n    needs: [publish_github_release]\n",
    )
elif name == "semver-baseline-needs-block-list":
    # Equivalent: the attach job's needs as a block sequence.
    replace_once(
        "    needs: [publish_github_release, publish_semver_baseline]\n",
        "    needs:\n      - publish_github_release\n      - publish_semver_baseline\n",
    )
elif name == "trailing-gates-critical":
    # The critical publisher waits for every lane again.
    replace_once(
        "    needs: [require_ci_green, release_validate_gate, release_semver_gate, build_binaries_critical_gate, release_prebuild]\n",
        "    needs: [require_ci_green, release_validate_gate, release_semver_gate, build_binaries_gate, release_prebuild]\n",
    )
elif name == "trailing-overwrites":
    # The critical upload replaces archives already on the release.
    replace_once(
        "            critical-release-artifacts/*.tar.gz\n"
        "          draft: false\n"
        "          fail_on_unmatched_files: true\n"
        "          generate_release_notes: true\n"
        "          overwrite_files: false\n",
        "            critical-release-artifacts/*.tar.gz\n"
        "          draft: false\n"
        "          fail_on_unmatched_files: true\n"
        "          generate_release_notes: true\n"
        "          overwrite_files: true\n",
    )
elif name == "trailing-selects-intel":
    # The critical set grows an Intel macOS archive.
    replace_once(
        '              "aarch64-apple-darwin",\n          )\n          wanted = sorted(\n',
        '              "aarch64-apple-darwin",\n              "x86_64-apple-darwin",\n          )\n'
        "          wanted = sorted(\n",
    )
elif name == "trailing-critical-if-reflowed":
    # Equivalent: the critical publisher's conditions in another order.
    replace_once(
        "      needs.release_semver_gate.result == 'success' &&\n"
        "      needs.build_binaries_critical_gate.result == 'success'\n",
        "      needs.build_binaries_critical_gate.result == 'success' &&\n"
        "      needs.release_semver_gate.result == 'success'\n",
    )
elif name == "prebuild-gate-waits-trailing":
    # The prebuild is reusable only once its trailing lanes finished too.
    replace_once(
        "    needs: [require_ci_green, build_binaries_critical_gate, build_web_sdk_package]\n",
        "    needs: [require_ci_green, build_binaries_gate, build_web_sdk_package]\n",
    )
elif name == "prebuild-reuse-no-wait":
    # The trailing publisher downloads before the prebuild's trailing lanes finished.
    match = re.search(
        r"      - name: Wait for the prebuild's trailing lanes\n"
        r"(?:        .*\n|          .*\n|\n)*?(?=      - name: |      # |\Z)",
        text,
    )
    if match is None:
        raise SystemExit(f"fixture `{name}` expects the trailing-lane wait step")
    text = text[: match.start()] + text[match.end() :]
elif name == "prebuild-critical-full-gate":
    # The critical publisher waits on the prebuild's full gate, which its
    # still-running trailing lanes hold back.
    replace_once('              --source-gate "Release critical binary build gate" \\\n', "")
elif name == "prebuild-web-ungated":
    # The pre-fix publish_web_sdk: a reused prebuild's package publishes
    # whatever release validation and the semver gate concluded.
    replace_once(
        "      (needs.release_validate_gate.result == 'success' ||\n"
        "       (github.event_name == 'workflow_dispatch' &&\n"
        "        github.event.inputs.publish_web_sdk_only == 'true' &&\n"
        "        needs.release_validate_gate.result == 'skipped')) &&\n"
        "      (needs.release_semver_gate.result == 'success' ||\n"
        "       (github.event_name == 'workflow_dispatch' &&\n"
        "        github.event.inputs.publish_web_sdk_only == 'true' &&\n"
        "        needs.release_semver_gate.result == 'skipped')) &&\n",
        "",
    )
elif name == "trailing-manifest-replaced":
    # The pre-review manifest upload: every run re-uploads it, replacing it.
    replace_once(
        "      - name: Publish complete release manifest\n"
        "        if: ${{ steps.manifest.outputs.upload == 'true' }}\n",
        "      - name: Publish complete release manifest\n",
    )
elif name == "token-write-default":
    # The pre-review default: every job gets a write token and OIDC.
    replace_once(
        "permissions:\n  actions: read\n  checks: read\n  contents: read\n",
        "permissions:\n  actions: read\n  checks: read\n  contents: write\n  id-token: write\n",
    )
elif name == "both-defects":
    replace_once(
        MEASUREMENT_STEP_IF,
        "      - name: Verify every reported break is named and the notes are stamped\n"
        "        run: make semver-breaks\n",
    )
    replace_once(SLO_LINE, SLO_LINE.replace("'1800'", "'3600'"))
else:
    raise SystemExit(f"unknown fixture `{name}`")

pathlib.Path(destination).write_text(text)
PYEOF
}

# run_check <workflow> <log> <check...>: prints the checker's exit status.
run_check() {
  local workflow="$1"
  local log="$2"
  shift 2
  set +e
  "$PYTHON" "$CHECKER" --workflow "$workflow" "$@" >"$log" 2>&1
  local status=$?
  set -e
  printf '%s' "$status"
}

expect_pass() {
  local label="$1"
  local fixture="$2"
  shift 2
  local log="${TEST_ROOT}/${label}.log"
  local status
  status="$(run_check "$fixture" "$log" "$@")"
  if [[ "$status" -ne 0 ]]; then
    fail "${label}: an equivalent workflow spelling was rejected" "$(cat "$log")"
  fi
}

expect_fail_named() {
  local label="$1"
  local fixture="$2"
  local needle="$3"
  shift 3
  local log="${TEST_ROOT}/${label}.log"
  local status
  status="$(run_check "$fixture" "$log" "$@")"
  if [[ "$status" -eq 0 ]]; then
    fail "${label}: a workflow that dropped the behaviour was accepted"
  fi
  if ! grep -Fq "$needle" "$log"; then
    fail "${label}: failure does not name the defect (expected: ${needle})" "$(cat "$log")"
  fi
}

# 1. The committed workflow satisfies both contracts.
expect_pass committed "$WORKFLOW" all

# 2. Equivalent spellings pass: this is the #1091 false positive.
mutate evidence-if-single-line "${TEST_ROOT}/evidence-if-single-line.yml"
expect_pass evidence-if-single-line "${TEST_ROOT}/evidence-if-single-line.yml" semver-evidence
mutate slo-literal "${TEST_ROOT}/slo-literal.yml"
expect_pass slo-literal "${TEST_ROOT}/slo-literal.yml" registry-slo
mutate slo-reflowed "${TEST_ROOT}/slo-reflowed.yml"
expect_pass slo-reflowed "${TEST_ROOT}/slo-reflowed.yml" registry-slo
mutate dispatch-reflowed "${TEST_ROOT}/dispatch-reflowed.yml"
expect_pass dispatch-reflowed "${TEST_ROOT}/dispatch-reflowed.yml" dispatch-binding
mutate dispatch-ref-name "${TEST_ROOT}/dispatch-ref-name.yml"
expect_pass dispatch-ref-name "${TEST_ROOT}/dispatch-ref-name.yml" dispatch-binding
mutate dispatch-ref-name-expression "${TEST_ROOT}/dispatch-ref-name-expression.yml"
expect_pass dispatch-ref-name-expression "${TEST_ROOT}/dispatch-ref-name-expression.yml" dispatch-binding
mutate tag-validation-reflowed "${TEST_ROOT}/tag-validation-reflowed.yml"
expect_pass tag-validation-reflowed "${TEST_ROOT}/tag-validation-reflowed.yml" tag-validation
mutate prebuild-lookup-if-reflowed "${TEST_ROOT}/prebuild-lookup-if-reflowed.yml"
expect_pass prebuild-lookup-if-reflowed "${TEST_ROOT}/prebuild-lookup-if-reflowed.yml" prebuild
mutate semver-baseline-needs-block-list "${TEST_ROOT}/semver-baseline-needs-block-list.yml"
expect_pass semver-baseline-needs-block-list "${TEST_ROOT}/semver-baseline-needs-block-list.yml" semver-baseline
mutate trailing-critical-if-reflowed "${TEST_ROOT}/trailing-critical-if-reflowed.yml"
expect_pass trailing-critical-if-reflowed "${TEST_ROOT}/trailing-critical-if-reflowed.yml" trailing-platforms

# 3. Dropping the behaviour fails and names the defect.
mutate evidence-step-removed "${TEST_ROOT}/evidence-step-removed.yml"
expect_fail_named evidence-step-removed "${TEST_ROOT}/evidence-step-removed.yml" \
  "no step that resolves the exact-tree" semver-evidence
mutate evidence-step-off-tags "${TEST_ROOT}/evidence-step-off-tags.yml"
expect_fail_named evidence-step-off-tags "${TEST_ROOT}/evidence-step-off-tags.yml" \
  "does not run on a tag push" semver-evidence
mutate measurement-on-tags "${TEST_ROOT}/measurement-on-tags.yml"
expect_fail_named measurement-on-tags "${TEST_ROOT}/measurement-on-tags.yml" \
  "reruns \`make semver-breaks\` on a tag push" semver-evidence
mutate slo-relaxed "${TEST_ROOT}/slo-relaxed.yml"
expect_fail_named slo-relaxed "${TEST_ROOT}/slo-relaxed.yml" \
  "passes \`--slo-seconds 3600\` on a tag push" registry-slo
mutate slo-flag-removed "${TEST_ROOT}/slo-flag-removed.yml"
expect_fail_named slo-flag-removed "${TEST_ROOT}/slo-flag-removed.yml" \
  "without \`--slo-seconds\`" registry-slo

mutate dispatch-unbound "${TEST_ROOT}/dispatch-unbound.yml"
expect_fail_named dispatch-unbound "${TEST_ROOT}/dispatch-unbound.yml" \
  "does not run on a package dispatch from refs/heads/main without release_tag" dispatch-binding
mutate dispatch-head-unchecked "${TEST_ROOT}/dispatch-head-unchecked.yml"
expect_fail_named dispatch-head-unchecked "${TEST_ROOT}/dispatch-head-unchecked.yml" \
  "(the tag is not the checked-out commit)" dispatch-binding
mutate dispatch-alpha-unscoped "${TEST_ROOT}/dispatch-alpha-unscoped.yml"
expect_fail_named dispatch-alpha-unscoped "${TEST_ROOT}/dispatch-alpha-unscoped.yml" \
  "accepts a package dispatch on the alpha/v0.0.0 tag without alpha_crates_only" dispatch-binding
mutate dispatch-prefers-github-ref "${TEST_ROOT}/dispatch-prefers-github-ref.yml"
expect_fail_named dispatch-prefers-github-ref "${TEST_ROOT}/dispatch-prefers-github-ref.yml" \
  "accepts a package dispatch on the v0.0.0 tag naming release_tag main (a branch)" dispatch-binding
mutate dispatch-alpha-any-version "${TEST_ROOT}/dispatch-alpha-any-version.yml"
expect_fail_named dispatch-alpha-any-version "${TEST_ROOT}/dispatch-alpha-any-version.yml" \
  "accepts an alpha crate dispatch on the alpha/v9.9.9 tag (another version)" dispatch-binding
mutate dispatch-branch-ref-name "${TEST_ROOT}/dispatch-branch-ref-name.yml"
expect_fail_named dispatch-branch-ref-name "${TEST_ROOT}/dispatch-branch-ref-name.yml" \
  "accepts a package dispatch from refs/heads/v0.0.0 without release_tag" dispatch-binding
mutate dispatch-branch-ref-name-tag-exists "${TEST_ROOT}/dispatch-branch-ref-name-tag-exists.yml"
expect_fail_named dispatch-branch-ref-name-tag-exists "${TEST_ROOT}/dispatch-branch-ref-name-tag-exists.yml" \
  "accepts a package dispatch from refs/heads/v0.0.0 without release_tag" dispatch-binding
mutate dispatch-prefers-github-ref-other-tag "${TEST_ROOT}/dispatch-prefers-github-ref-other-tag.yml"
expect_fail_named dispatch-prefers-github-ref-other-tag "${TEST_ROOT}/dispatch-prefers-github-ref-other-tag.yml" \
  "(a tag on another commit)" dispatch-binding
mutate dispatch-prefers-github-ref-head-checked "${TEST_ROOT}/dispatch-prefers-github-ref-head-checked.yml"
expect_fail_named dispatch-prefers-github-ref-head-checked "${TEST_ROOT}/dispatch-prefers-github-ref-head-checked.yml" \
  "(both tags on one commit, no alpha lane)" dispatch-binding
mutate dispatch-prefix-glob "${TEST_ROOT}/dispatch-prefix-glob.yml"
expect_fail_named dispatch-prefix-glob "${TEST_ROOT}/dispatch-prefix-glob.yml" \
  "(a near miss of v0.0.0)" dispatch-binding
mutate dispatch-alpha-named-any-version "${TEST_ROOT}/dispatch-alpha-named-any-version.yml"
expect_fail_named dispatch-alpha-named-any-version "${TEST_ROOT}/dispatch-alpha-named-any-version.yml" \
  "accepts an alpha crate dispatch naming release_tag alpha/v9.9.9 (another version)" dispatch-binding
mutate dispatch-skips-named-tag-run "${TEST_ROOT}/dispatch-skips-named-tag-run.yml"
expect_fail_named dispatch-skips-named-tag-run "${TEST_ROOT}/dispatch-skips-named-tag-run.yml" \
  "does not run on a package dispatch on the v0.0.0 tag naming release_tag main (a branch)" dispatch-binding
mutate dispatch-checkout-github-ref "${TEST_ROOT}/dispatch-checkout-github-ref.yml"
expect_fail_named dispatch-checkout-github-ref "${TEST_ROOT}/dispatch-checkout-github-ref.yml" \
  "refuses a package dispatch naming release_tag v0.0.0" dispatch-binding
mutate dispatch-branch-name "${TEST_ROOT}/dispatch-branch-name.yml"
expect_fail_named dispatch-branch-name "${TEST_ROOT}/dispatch-branch-name.yml" \
  "accepts a package dispatch from refs/heads/release/v0.0.0 without release_tag" dispatch-binding

mutate tag-validation-cargo-on-attested-tags "${TEST_ROOT}/tag-validation-cargo-on-attested-tags.yml"
expect_fail_named tag-validation-cargo-on-attested-tags "${TEST_ROOT}/tag-validation-cargo-on-attested-tags.yml" \
  "runs on a tag push whose attestation covers the Cargo lanes (true)" tag-validation
mutate tag-validation-gate-any-schema "${TEST_ROOT}/tag-validation-gate-any-schema.yml"
expect_fail_named tag-validation-gate-any-schema "${TEST_ROOT}/tag-validation-gate-any-schema.yml" \
  "accepts a tag push whose attestation does not cover the Cargo lanes and Cargo validation skipped" tag-validation
mutate tag-validation-schema-unwritten "${TEST_ROOT}/tag-validation-schema-unwritten.yml"
expect_fail_named tag-validation-schema-unwritten "${TEST_ROOT}/tag-validation-schema-unwritten.yml" \
  "writes covers_release_validation=<nothing> where every lane ran" tag-validation
mutate tag-validation-ignores-sdk-host "${TEST_ROOT}/tag-validation-ignores-sdk-host.yml"
expect_fail_named tag-validation-ignores-sdk-host "${TEST_ROOT}/tag-validation-ignores-sdk-host.yml" \
  "writes covers_release_validation=true where the SDK suites were skipped" tag-validation

mutate prebuild-publishes "${TEST_ROOT}/prebuild-publishes.yml"
expect_fail_named prebuild-publishes "${TEST_ROOT}/prebuild-publishes.yml" \
  "job \`publish_github_release\` publishes on a prebuild dispatch on main" prebuild
mutate prebuild-moving-checkout "${TEST_ROOT}/prebuild-moving-checkout.yml"
expect_fail_named prebuild-moving-checkout "${TEST_ROOT}/prebuild-moving-checkout.yml" \
  "job \`build_web_sdk_package\` checks out \`refs/heads/main\` on a prebuild dispatch on main" prebuild
mutate prebuild-builds-anyway "${TEST_ROOT}/prebuild-builds-anyway.yml"
expect_fail_named prebuild-builds-anyway "${TEST_ROOT}/prebuild-builds-anyway.yml" \
  "job \`build_binaries_windows_cross\` builds on a tag push with a prebuild found" prebuild
mutate prebuild-reuse-unverified "${TEST_ROOT}/prebuild-reuse-unverified.yml"
expect_fail_named prebuild-reuse-unverified "${TEST_ROOT}/prebuild-reuse-unverified.yml" \
  "expected \`scripts/verify-release-asset-provenance --source-run 424242 --source-attempt 2 --prebuild --stamps ...\`" prebuild
mutate prebuild-web-unverified "${TEST_ROOT}/prebuild-web-unverified.yml"
expect_fail_named prebuild-web-unverified "${TEST_ROOT}/prebuild-web-unverified.yml" \
  "must verify the prebuilt package's attestation" prebuild
mutate prebuild-reuse-no-wait "${TEST_ROOT}/prebuild-reuse-no-wait.yml"
expect_fail_named prebuild-reuse-no-wait "${TEST_ROOT}/prebuild-reuse-no-wait.yml" \
  "must wait for the prebuild run's \`Release binary build gate\`" prebuild
mutate prebuild-gate-waits-trailing "${TEST_ROOT}/prebuild-gate-waits-trailing.yml"
expect_fail_named prebuild-gate-waits-trailing "${TEST_ROOT}/prebuild-gate-waits-trailing.yml" \
  "job \`release_prebuild_gate\` waits for \`build_binaries_gate\`" trailing-platforms
mutate prebuild-critical-full-gate "${TEST_ROOT}/prebuild-critical-full-gate.yml"
expect_fail_named prebuild-critical-full-gate "${TEST_ROOT}/prebuild-critical-full-gate.yml" \
  "expected \`Release critical binary build gate\`" prebuild
mutate prebuild-web-ungated "${TEST_ROOT}/prebuild-web-ungated.yml"
expect_fail_named prebuild-web-ungated "${TEST_ROOT}/prebuild-web-ungated.yml" \
  "job \`publish_web_sdk\` publishes on a tag push with a prebuild found and \`release_validate_gate\` failed" prebuild
mutate prebuild-gate-renamed "${TEST_ROOT}/prebuild-gate-renamed.yml"
expect_fail_named prebuild-gate-renamed "${TEST_ROOT}/prebuild-gate-renamed.yml" \
  "does not require the \`Prebuild complete\` job" prebuild

mutate semver-baseline-gates-homebrew "${TEST_ROOT}/semver-baseline-gates-homebrew.yml"
expect_fail_named semver-baseline-gates-homebrew "${TEST_ROOT}/semver-baseline-gates-homebrew.yml" \
  "job \`update_homebrew\` waits for the semver baseline rustdoc" semver-baseline
mutate semver-baseline-after-release "${TEST_ROOT}/semver-baseline-after-release.yml"
expect_fail_named semver-baseline-after-release "${TEST_ROOT}/semver-baseline-after-release.yml" \
  "the rustdoc build must run in parallel with publication, not after it" semver-baseline

mutate trailing-gates-critical "${TEST_ROOT}/trailing-gates-critical.yml"
expect_fail_named trailing-gates-critical "${TEST_ROOT}/trailing-gates-critical.yml" \
  "waits for \`build_binaries_gate\`, \`build_binaries_macos_x86_cross\`" trailing-platforms
mutate trailing-overwrites "${TEST_ROOT}/trailing-overwrites.yml"
expect_fail_named trailing-overwrites "${TEST_ROOT}/trailing-overwrites.yml" \
  "release uploads are add-only" trailing-platforms
mutate trailing-manifest-replaced "${TEST_ROOT}/trailing-manifest-replaced.yml"
expect_fail_named trailing-manifest-replaced "${TEST_ROOT}/trailing-manifest-replaced.yml" \
  "writes the manifest unconditionally" trailing-platforms
mutate trailing-selects-intel "${TEST_ROOT}/trailing-selects-intel.yml"
expect_fail_named trailing-selects-intel "${TEST_ROOT}/trailing-selects-intel.yml" \
  "rkat-0.0.0-x86_64-apple-darwin.tar.gz" trailing-platforms

mutate token-write-default "${TEST_ROOT}/token-write-default.yml"
expect_fail_named token-write-default "${TEST_ROOT}/token-write-default.yml" \
  "the workflow default grants \`contents: write\` to every job" token-permissions

mutate readback-step-removed "${TEST_ROOT}/readback-step-removed.yml"
expect_fail_named readback-step-removed "${TEST_ROOT}/readback-step-removed.yml" \
  "no step that reads every crate back" registry-slo
mutate slo-before-sdk "${TEST_ROOT}/slo-before-sdk.yml"
expect_fail_named slo-before-sdk "${TEST_ROOT}/slo-before-sdk.yml" \
  "SDK publication must not wait on the publication SLO" registry-slo

# 4. The doctor itself binds to the checker: its PASS/FAIL lines follow the
#    workflow it is pointed at. Environment checks (gh, npm) are ignored; only
#    the two workflow-contract lines are asserted.
run_doctor() {
  local workflow="$1"
  local log="$2"
  set +e
  RELEASE_WORKFLOW_FILE="$workflow" "$DOCTOR" >"$log" 2>&1
  set -e
}

run_doctor "$WORKFLOW" "${TEST_ROOT}/doctor-committed.log"
for line in "$SEMVER_PASS" "$SLO_PASS" "$DISPATCH_PASS" "$TAG_VALIDATION_PASS" "$PREBUILD_PASS" "$BASELINE_PASS" "$TRAILING_PASS"; do
  if ! grep -Fxq "$line" "${TEST_ROOT}/doctor-committed.log"; then
    fail "doctor does not report \`${line}\` for the committed workflow" \
      "$(cat "${TEST_ROOT}/doctor-committed.log")"
  fi
done

mutate both-defects "${TEST_ROOT}/both-defects.yml"
run_doctor "${TEST_ROOT}/both-defects.yml" "${TEST_ROOT}/doctor-defects.log"
for line in "$SEMVER_FAIL" "$SLO_FAIL"; do
  if ! grep -Fq "$line" "${TEST_ROOT}/doctor-defects.log"; then
    fail "doctor does not report \`${line}\` for a workflow that dropped the behaviour" \
      "$(cat "${TEST_ROOT}/doctor-defects.log")"
  fi
done

mutate dispatch-unbound "${TEST_ROOT}/doctor-dispatch-unbound.yml"
run_doctor "${TEST_ROOT}/doctor-dispatch-unbound.yml" "${TEST_ROOT}/doctor-dispatch.log"
if ! grep -Fq "$DISPATCH_FAIL" "${TEST_ROOT}/doctor-dispatch.log"; then
  fail "doctor does not report \`${DISPATCH_FAIL}\` for a workflow whose branch dispatch is unbound" \
    "$(cat "${TEST_ROOT}/doctor-dispatch.log")"
fi

mutate tag-validation-cargo-on-attested-tags "${TEST_ROOT}/doctor-tag-validation.yml"
run_doctor "${TEST_ROOT}/doctor-tag-validation.yml" "${TEST_ROOT}/doctor-tag-validation.log"
if ! grep -Fq "$TAG_VALIDATION_FAIL" "${TEST_ROOT}/doctor-tag-validation.log"; then
  fail "doctor does not report \`${TAG_VALIDATION_FAIL}\` for a workflow that re-runs Cargo validation on attested tags" \
    "$(cat "${TEST_ROOT}/doctor-tag-validation.log")"
fi

mutate prebuild-publishes "${TEST_ROOT}/doctor-prebuild.yml"
run_doctor "${TEST_ROOT}/doctor-prebuild.yml" "${TEST_ROOT}/doctor-prebuild.log"
if ! grep -Fq "$PREBUILD_FAIL" "${TEST_ROOT}/doctor-prebuild.log"; then
  fail "doctor does not report \`${PREBUILD_FAIL}\` for a workflow whose prebuild publishes" \
    "$(cat "${TEST_ROOT}/doctor-prebuild.log")"
fi

mutate semver-baseline-gates-homebrew "${TEST_ROOT}/doctor-baseline.yml"
run_doctor "${TEST_ROOT}/doctor-baseline.yml" "${TEST_ROOT}/doctor-baseline.log"
if ! grep -Fq "$BASELINE_FAIL" "${TEST_ROOT}/doctor-baseline.log"; then
  fail "doctor does not report \`${BASELINE_FAIL}\` for a workflow that waits for the baseline" \
    "$(cat "${TEST_ROOT}/doctor-baseline.log")"
fi

mutate trailing-gates-critical "${TEST_ROOT}/doctor-trailing.yml"
run_doctor "${TEST_ROOT}/doctor-trailing.yml" "${TEST_ROOT}/doctor-trailing.log"
if ! grep -Fq "$TRAILING_FAIL" "${TEST_ROOT}/doctor-trailing.log"; then
  fail "doctor does not report \`${TRAILING_FAIL}\` for a workflow whose critical assets wait for the trailing lanes" \
    "$(cat "${TEST_ROOT}/doctor-trailing.log")"
fi

echo "release doctor workflow contract holds"
