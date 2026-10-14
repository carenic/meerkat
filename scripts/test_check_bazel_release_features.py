#!/usr/bin/env python3
"""Contract test for scripts/check-bazel-release-features.py.

The release feature gate must pass a clean release graph and fail, naming the
defect, on each way test-only code can reach a Bazel release binary: a select
whose release branch keeps a non-shipping feature, a plain feature list or a
dependency forward carrying one, a platform select whose branch carries one
(with or without a release branch beside it), a select, feature list,
dependency list or alias it cannot read whole, an alias or proc macro on the
path, a *_test_support variant in the closure, a
release build without the define, a -c opt build outside the launcher, a
config_setting that does not match the define, and release roots that
disagree with what the workflow packages.
"""

from __future__ import annotations

import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECKER = ROOT / "scripts" / "check-bazel-release-features.py"

FEATURES = ["integration-real-tests", "test-support"]

LAUNCHER = """#!/usr/bin/env bash
case "${command}" in
  workspace-test)
    command="test"
    default_target="//..."
    ;;
  release-build-linux-x86)
    command="build"
    default_target="//app:app_bin //tool:tool"
    extra_args+=(--compilation_mode=opt --define=meerkat_release_build=1 --remote_download_outputs=toplevel)
    ;;
  release-build-windows-x86)
    command="build"
    default_target="//app:app_bin //tool:tool"
    extra_args+=(
      --compilation_mode=opt
      --define=meerkat_release_build=1
    )
    ;;
esac
"""

WORKFLOW = """jobs:
  build_binaries_buildbuddy:
    strategy:
      matrix:
        include:
          - bb_command: release-build-linux-x86
    steps:
      - run: |
          declare -A BIN_PATHS=(
            [app]="app/app_bin"
            [tool]="tool/tool"
          )
"""

SETTING = """config_setting(
    name = "release_build",
    define_values = {"meerkat_release_build": "1"},
    visibility = ["//visibility:public"],
)
"""

APP = """rust_library(
    name = "app",
    crate_features = select({
        "//tools/bazel:release_build": [
            "default",
        ],
        "//conditions:default": [
            "default",
            "test-support",
        ],
    }),
    deps = [
        "//core:core",
    ] + all_crate_deps(
        package_name = "app",
        normal = True,
    ),
)

rust_binary(
    name = "app_bin",
    crate_features = [],
    deps = [
        ":app",
        "//core:core_agent_factory_build",
    ] + all_crate_deps(
        package_name = "app",
        normal = True,
    ),
    proc_macro_deps = [
        "//macros:macros",
    ],
)

rust_library(
    name = "app_with_runtime_test_support",
    crate_features = [
        "default",
        "test-support",
    ],
    deps = [
        "//core:core_test_support",
    ],
)
"""

CORE = """rust_library(
    name = "core",
    crate_features = select({
        "//tools/bazel:release_build": [],
        "//conditions:default": [
            "integration-real-tests",
        ],
    }),
    deps = [],
)

alias(
    name = "core_agent_factory_build",
    actual = "//core:core",
)

rust_library(
    name = "core_test_support",
    crate_features = [
        "integration-real-tests",
    ],
    deps = [],
)
"""

TOOL = """rust_binary(
    name = "tool",
    crate_features = [
        "cli",
    ],
    deps = [
        "//core",
    ],
)
"""

MACROS = """rust_proc_macro(
    name = "macros",
    crate_features = [],
    deps = [],
)
"""


class ReleaseFeatureGateTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory(prefix="meerkat-release-features-")
        self.root = pathlib.Path(self._tmp.name)
        files = {
            "scripts/non-shipping-features.json": json.dumps({"features": FEATURES}),
            "scripts/buildbuddy-bazel-poc": LAUNCHER,
            ".github/workflows/release.yml": WORKFLOW,
            "tools/bazel/BUILD.bazel": SETTING,
            "app/BUILD.bazel": APP,
            "core/BUILD.bazel": CORE,
            "tool/BUILD.bazel": TOOL,
            "macros/BUILD.bazel": MACROS,
            "Makefile": "build:\n\tscripts/buildbuddy-bazel-poc\n",
        }
        for rel, text in files.items():
            path = self.root / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8")

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def edit(self, rel: str, old: str, new: str) -> None:
        path = self.root / rel
        text = path.read_text(encoding="utf-8")
        self.assertEqual(text.count(old), 1, f"fixture edit expects exactly one {old!r} in {rel}")
        path.write_text(text.replace(old, new), encoding="utf-8")

    def run_checker(self, root: pathlib.Path | None = None) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(CHECKER), str(root or self.root)],
            capture_output=True,
            text=True,
            check=False,
        )

    def assert_fails(self, *needles: str) -> None:
        result = self.run_checker()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        for needle in needles:
            self.assertIn(needle, result.stderr)

    def test_clean_fixture_passes(self) -> None:
        result = self.run_checker()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("//app:app_bin, //tool:tool", result.stdout)
        self.assertIn("2 release selects evaluated", result.stdout)

    def test_release_branch_keeping_a_non_shipping_feature_fails(self) -> None:
        self.edit("app/BUILD.bazel", '"//tools/bazel:release_build": [\n            "default",\n', '"//tools/bazel:release_build": [\n            "default",\n            "test-support",\n')
        self.assert_fails("//app:app enables non-shipping feature 'test-support' in the release configuration")

    def test_select_without_release_branch_checks_its_default_branch(self) -> None:
        self.edit("core/BUILD.bazel", '        "//tools/bazel:release_build": [],\n', "")
        self.assert_fails("//core:core enables non-shipping feature 'integration-real-tests'")

    CORE_SELECT = (
        '        "//tools/bazel:release_build": [],\n'
        '        "//conditions:default": [\n'
        '            "integration-real-tests",\n'
        '        ],\n'
    )

    def test_platform_select_branch_carrying_a_non_shipping_feature_fails(self) -> None:
        # A Linux-only test-support feature must not escape behind a clean
        # default branch: every alternative of a select without the release
        # key is checked.
        self.edit(
            "core/BUILD.bazel",
            self.CORE_SELECT,
            '        "@platforms//os:linux": [\n'
            '            "integration-real-tests",\n'
            '        ],\n'
            '        "//conditions:default": [],\n',
        )
        self.assert_fails("//core:core enables non-shipping feature 'integration-real-tests'")

    def test_platform_branch_beside_the_release_branch_fails(self) -> None:
        self.edit(
            "core/BUILD.bazel",
            self.CORE_SELECT,
            '        "//tools/bazel:release_build": [],\n'
            '        "@platforms//os:linux": [\n'
            '            "integration-real-tests",\n'
            '        ],\n'
            '        "//conditions:default": [],\n',
        )
        self.assert_fails("//core:core enables non-shipping feature 'integration-real-tests'")

    def test_clean_platform_select_passes(self) -> None:
        self.edit(
            "core/BUILD.bazel",
            self.CORE_SELECT,
            '        "@platforms//os:linux": [\n'
            '            "linux-only",\n'
            '        ],\n'
            '        "//conditions:default": [],\n',
        )
        result = self.run_checker()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_select_it_cannot_evaluate_fails_closed(self) -> None:
        self.edit(
            "core/BUILD.bazel",
            self.CORE_SELECT,
            '        "@platforms//os:linux": LINUX_FEATURES,\n'
            '        "//conditions:default": [],\n',
        )
        result = self.run_checker()
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertIn("cannot evaluate select() branch '@platforms//os:linux'", result.stderr)

    def test_concatenation_inside_a_release_feature_branch_is_checked(self) -> None:
        # The whole branch value counts, not just its first list.
        self.edit(
            "app/BUILD.bazel",
            '        "//tools/bazel:release_build": [\n            "default",\n        ],\n',
            '        "//tools/bazel:release_build": [\n            "default",\n        ] + ["test-support"],\n',
        )
        self.assert_fails("//app:app enables non-shipping feature 'test-support'")

    def test_concatenation_inside_a_release_deps_branch_is_checked(self) -> None:
        self.edit(
            "tool/BUILD.bazel",
            '    deps = [\n        "//core",\n    ],\n',
            '    deps = select({\n'
            '        "//tools/bazel:release_build": ["//core"] + ["//core:core_test_support"],\n'
            '        "//conditions:default": ["//core"],\n'
            '    }),\n',
        )
        self.assert_fails(
            "//core:core_test_support is a test-support variant in the release closure",
            "//core:core_test_support enables non-shipping feature 'integration-real-tests'",
        )

    def test_unsupported_syntax_inside_a_branch_fails_closed(self) -> None:
        self.edit(
            "core/BUILD.bazel",
            self.CORE_SELECT,
            '        "//tools/bazel:release_build": [] + RELEASE_FEATURES,\n'
            '        "//conditions:default": [],\n',
        )
        result = self.run_checker()
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertIn("cannot evaluate select() branch", result.stderr)

    def test_identifier_inside_a_release_feature_branch_fails_closed(self) -> None:
        self.edit(
            "app/BUILD.bazel",
            '        "//tools/bazel:release_build": [\n            "default",\n        ],\n',
            '        "//tools/bazel:release_build": [\n            "default",\n            EXTRA_FEATURES,\n        ],\n',
        )
        result = self.run_checker()
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertIn("//app:app: cannot evaluate select() branch '//tools/bazel:release_build'", result.stderr)

    def test_dependency_term_it_cannot_read_fails_closed(self) -> None:
        self.edit("tool/BUILD.bazel", '    deps = [\n        "//core",\n    ],\n', '    deps = [\n        "//core",\n    ] + EXTRA_DEPS,\n')
        result = self.run_checker()
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertIn("//tool:tool deps: cannot evaluate", result.stderr)

    def test_alias_it_cannot_evaluate_fails_closed(self) -> None:
        self.edit("core/BUILD.bazel", '    actual = "//core:core",\n', "    actual = CORE_LABEL,\n")
        result = self.run_checker()
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertIn("//core:core_agent_factory_build: cannot evaluate alias actual 'CORE_LABEL'", result.stderr)

    def test_feature_list_sum_with_a_select_is_checked(self) -> None:
        self.edit("tool/BUILD.bazel", '    crate_features = [\n        "cli",\n    ],\n', '    crate_features = ["cli"] + select({\n        "@platforms//os:macos": ["test-support"],\n        "//conditions:default": [],\n    }),\n')
        self.assert_fails("//tool:tool enables non-shipping feature 'test-support'")

    def test_plain_list_on_a_binary_fails(self) -> None:
        self.edit("tool/BUILD.bazel", '"cli",\n', '"cli",\n        "integration-real-tests",\n')
        self.assert_fails("//tool:tool enables non-shipping feature 'integration-real-tests'")

    def test_dependency_forward_fails(self) -> None:
        self.edit("tool/BUILD.bazel", '"cli",\n', '"cli",\n        "core?/test-support",\n')
        self.assert_fails("//tool:tool enables non-shipping feature 'core?/test-support'")

    def test_proc_macro_on_the_path_is_checked(self) -> None:
        self.edit("macros/BUILD.bazel", "crate_features = [],", 'crate_features = ["test-support"],')
        self.assert_fails("//macros:macros enables non-shipping feature 'test-support'")

    def test_test_support_variant_in_the_closure_fails(self) -> None:
        self.edit("app/BUILD.bazel", '        ":app",\n', '        ":app_with_runtime_test_support",\n')
        self.assert_fails(
            "//app:app_with_runtime_test_support is a test-support variant in the release closure",
            "//core:core_test_support is a test-support variant in the release closure",
        )

    def test_unknown_label_fails_closed(self) -> None:
        self.edit("tool/BUILD.bazel", '"//core",', '"//missing:lib",')
        self.assert_fails("//missing:lib is in the release closure but no BUILD.bazel defines it")

    def test_release_build_without_the_define_fails(self) -> None:
        self.edit(
            "scripts/buildbuddy-bazel-poc",
            "--compilation_mode=opt --define=meerkat_release_build=1 --remote",
            "--compilation_mode=opt --remote",
        )
        self.assert_fails("release-build-linux-x86 builds release binaries without --define=meerkat_release_build=1")

    def test_opt_build_outside_the_launcher_fails(self) -> None:
        self.edit("Makefile", "\tscripts/buildbuddy-bazel-poc\n", "\tbb build -c opt //app:app_bin\n")
        self.assert_fails("Makefile:2 builds with -c opt outside the scripts/buildbuddy-bazel-poc release-build cases")

    def test_setting_that_does_not_match_the_define_fails(self) -> None:
        self.edit("tools/bazel/BUILD.bazel", '"meerkat_release_build": "1"', '"meerkat_release": "1"')
        self.assert_fails("//tools/bazel:release_build must match exactly --define=meerkat_release_build=1")

    def test_workflow_packaging_other_roots_fails(self) -> None:
        self.edit(".github/workflows/release.yml", '[tool]="tool/tool"', '[tool]="tool/other"')
        self.assert_fails("packages ['//app:app_bin', '//tool:other'] but")

    def test_workflow_command_outside_the_checked_cases_fails(self) -> None:
        self.edit(".github/workflows/release.yml", "bb_command: release-build-linux-x86", "bb_command: workspace-test")
        self.assert_fails("builds release binaries with workspace-test, not a checked release-build-* case")

    def test_release_cases_must_build_the_same_roots(self) -> None:
        self.edit("scripts/buildbuddy-bazel-poc", 'default_target="//app:app_bin //tool:tool"\n    extra_args+=(\n', 'default_target="//app:app_bin"\n    extra_args+=(\n')
        self.assert_fails("release-build-windows-x86 builds ['//app:app_bin']")

    def test_repository_release_graph_is_clean(self) -> None:
        result = self.run_checker(ROOT)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("//crates/meerkat-cli:rkat", result.stdout)


if __name__ == "__main__":
    unittest.main()
