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
from pathlib import Path

SECTION_RE = re.compile(r"^## \[([^\]]+)\](.*)$")
STAMPED_SUFFIX_RE = re.compile(r"^\s*-\s*\d{4}-\d{2}-\d{2}\s*$")


def fail(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


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

    stamped_heading = f"## [{release_version}] - {release_date}"
    lines[first + 1 : first + 1] = ["", stamped_heading]
    changelog_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"  Stamped CHANGELOG.md as `{stamped_heading}` (empty [Unreleased] stub retained)")


if __name__ == "__main__":
    main()
