#!/usr/bin/env python3
"""Stamp the pending `## [Unreleased]` CHANGELOG.md section for a release.

WHY THIS EXISTS AS A RELEASE STEP AND NOT A CHECKLIST LINE
----------------------------------------------------------
`scripts/check_semver_breaks.py` requires the section a release is declared in
to carry the version being released and a `- YYYY-MM-DD` date. Nothing used to
produce that, and the two obvious ways to do it by hand are both wrong:

  * stamp BEFORE the version bump, and `check_stamped` fails with "the release
    notes are declared against a different version than the one being
    released" - the notes say 0.8.24 while the workspace still says 0.8.23;
  * stamp AFTER the bump lands, and the tag publishes release notes titled
    "Unreleased".

The only place the stamp and the bump can be the same commit is this hook,
which cargo-release runs after it rewrites the version and before it commits.
So the stamp is owned here, once, rather than remembered by whoever is holding
the release.

The stamp leaves an EMPTY `## [Unreleased]` stub above the stamped section,
which is the shape `pending_section()` expects: it skips a content-free
topmost section and reads the one below it.

FAILS CLOSED. A release whose notes are missing or already stamped for another
version is a release whose notes nobody can trust; this refuses rather than
inventing a heading.
"""

from __future__ import annotations

import datetime
import os
import re
import sys
import tempfile
from pathlib import Path

SECTION_RE = re.compile(r"^## \[([^\]]+)\](.*)$")
STAMPED_SUFFIX_RE = re.compile(r"^\s*-\s*\d{4}-\d{2}-\d{2}\s*$")
REFERENCE_RE = re.compile(r"^\[([^\]]+)\]:\s*(\S+)\s*$")
UNRELEASED_COMPARE_RE = re.compile(r"^(?P<prefix>.+/compare/)v(?P<base>[^/]+?)\.\.\.HEAD$")


def fail(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


def release_label_at(lines: list[str], index: int) -> str:
    match = SECTION_RE.match(lines[index])
    if match is None:
        fail(f"internal error: expected release heading at line {index + 1}")
    return match.group(1).strip()


def reference_positions(lines: list[str]) -> dict[str, int]:
    positions: dict[str, int] = {}
    for index, line in enumerate(lines):
        match = REFERENCE_RE.match(line)
        if match is None:
            continue
        label = match.group(1).strip()
        if label in positions:
            fail(f"CHANGELOG.md contains duplicate comparison reference [{label}]")
        positions[label] = index
    return positions


def prepare_comparison_links(
    lines: list[str], release_version: str, previous_version: str
) -> None:
    positions = reference_positions(lines)
    unreleased_index = positions.get("Unreleased")
    if unreleased_index is None:
        fail("CHANGELOG.md is missing its [Unreleased] comparison reference")
    previous_index = positions.get(previous_version)
    if previous_index is None:
        fail(
            f"CHANGELOG.md is missing the previous release comparison reference "
            f"[{previous_version}]"
        )
    if release_version in positions:
        fail(
            f"CHANGELOG.md already contains comparison reference [{release_version}] "
            "before its release heading was stamped"
        )

    current = REFERENCE_RE.match(lines[unreleased_index])
    assert current is not None
    compare = UNRELEASED_COMPARE_RE.match(current.group(2))
    if compare is None:
        fail(
            "the [Unreleased] comparison reference must end in "
            f"/compare/v{previous_version}...HEAD"
        )
    if compare.group("base") != previous_version:
        fail(
            "the [Unreleased] comparison reference starts at "
            f"v{compare.group('base')}, expected v{previous_version}"
        )

    prefix = compare.group("prefix")
    lines[unreleased_index] = (
        f"[Unreleased]: {prefix}v{release_version}...HEAD"
    )
    lines.insert(
        unreleased_index + 1,
        f"[{release_version}]: {prefix}v{previous_version}...v{release_version}",
    )


def validate_stamped_comparison_links(
    lines: list[str], release_version: str, previous_version: str
) -> None:
    positions = reference_positions(lines)
    unreleased_index = positions.get("Unreleased")
    release_index = positions.get(release_version)
    if unreleased_index is None or release_index is None:
        fail(
            f"CHANGELOG.md is stamped for {release_version} but its comparison "
            "references are incomplete"
        )

    unreleased = REFERENCE_RE.match(lines[unreleased_index])
    released = REFERENCE_RE.match(lines[release_index])
    assert unreleased is not None and released is not None
    compare = UNRELEASED_COMPARE_RE.match(unreleased.group(2))
    if compare is None or compare.group("base") != release_version:
        fail(
            f"[Unreleased] must compare v{release_version}...HEAD after stamping"
        )
    expected_release = (
        f"{compare.group('prefix')}v{previous_version}...v{release_version}"
    )
    if released.group(2) != expected_release:
        fail(
            f"[{release_version}] must compare "
            f"v{previous_version}...v{release_version}"
        )


def write_atomic(path: Path, lines: list[str]) -> None:
    mode = path.stat().st_mode
    temporary_name: str | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=path.parent,
            prefix=f".{path.name}.",
            suffix=".tmp",
            delete=False,
        ) as temporary:
            temporary_name = temporary.name
            temporary.write("\n".join(lines) + "\n")
            temporary.flush()
            os.fsync(temporary.fileno())
        assert temporary_name is not None
        os.chmod(temporary_name, mode)
        os.replace(temporary_name, path)
    finally:
        if temporary_name is not None and os.path.exists(temporary_name):
            os.unlink(temporary_name)


def main() -> None:
    if len(sys.argv) != 3:
        fail("usage: stamp-changelog-release.py <changelog-path> <version>")

    changelog_file = Path(sys.argv[1])
    release_version = sys.argv[2].strip()
    if not release_version:
        fail("refusing to stamp an empty version")

    # Overridable so the contract test can assert an exact heading.
    release_date = os.environ.get("MEERKAT_RELEASE_DATE") or datetime.datetime.now(
        datetime.timezone.utc
    ).strftime("%Y-%m-%d")
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", release_date):
        fail(f"MEERKAT_RELEASE_DATE must be YYYY-MM-DD, got {release_date!r}")

    text = changelog_file.read_text(encoding="utf-8")
    lines = text.splitlines()

    heading_positions = [i for i, line in enumerate(lines) if SECTION_RE.match(line)]
    if not heading_positions:
        fail(f"{changelog_file} has no `## [...]` section at all")

    first = heading_positions[0]
    match = SECTION_RE.match(lines[first])
    assert match is not None  # guarded by heading_positions
    label, suffix = match.group(1).strip(), match.group(2)

    def is_stamp_for_release(index: int) -> bool:
        candidate = SECTION_RE.match(lines[index])
        return bool(
            candidate
            and candidate.group(1).strip() == release_version
            and STAMPED_SUFFIX_RE.match(candidate.group(2))
        )

    if label.lower() != "unreleased":
        # No `[Unreleased]` stub at all - only idempotent if THIS is our stamp.
        if is_stamp_for_release(first):
            if len(heading_positions) < 2:
                fail(
                    f"cannot determine the release before {release_version} "
                    "for comparison links"
                )
            validate_stamped_comparison_links(
                lines, release_version, release_label_at(lines, heading_positions[1])
            )
            print(f"  CHANGELOG.md already stamped for {release_version}; nothing to do")
            return
        fail(
            f"the topmost {changelog_file} section is `{lines[first].strip()}`, which is "
            f"neither `## [Unreleased]` nor an existing stamp for {release_version}. "
            f"Refusing to guess where the notes for this release are."
        )

    # Body of the pending section: everything up to the next `## [` heading.
    body_end = heading_positions[1] if len(heading_positions) > 1 else len(lines)
    if not "\n".join(lines[first + 1 : body_end]).strip():
        # A successful stamp LEAVES this stub empty, so "empty stub" is the shape
        # of both "already done" and "no notes were written". Distinguish them by
        # what sits below it - the same skip-the-stub rule the break gate applies -
        # or re-running the hook reports missing notes for work it just stamped.
        if len(heading_positions) > 1 and is_stamp_for_release(heading_positions[1]):
            if len(heading_positions) < 3:
                fail(
                    f"cannot determine the release before {release_version} "
                    "for comparison links"
                )
            validate_stamped_comparison_links(
                lines, release_version, release_label_at(lines, heading_positions[2])
            )
            print(f"  CHANGELOG.md already stamped for {release_version}; nothing to do")
            return
        fail(
            f"the pending `## [Unreleased]` section in {changelog_file} is empty. A release "
            f"with no notes would publish an empty section and the break gate would read "
            f"the PREVIOUS release as this one's declaration. Write the notes first."
        )

    already = [
        lines[i]
        for i in heading_positions[1:]
        if (m := SECTION_RE.match(lines[i])) and m.group(1).strip() == release_version
    ]
    if already:
        fail(
            f"{changelog_file} already contains a section for {release_version} "
            f"(`{already[0].strip()}`) below the pending one. Stamping again would declare "
            f"the same version twice."
        )

    if len(heading_positions) < 2:
        fail(
            f"cannot determine the release before {release_version} for comparison links"
        )
    previous_version = release_label_at(lines, heading_positions[1])
    prepare_comparison_links(lines, release_version, previous_version)

    stamped_heading = f"## [{release_version}] - {release_date}"
    lines[first + 1 : first + 1] = ["", stamped_heading]
    write_atomic(changelog_file, lines)
    print(
        f"  Stamped CHANGELOG.md as `{stamped_heading}`, advanced comparison links "
        "(empty [Unreleased] stub retained)"
    )


if __name__ == "__main__":
    main()
