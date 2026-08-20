#!/usr/bin/env python3
"""Regression tests for the local Mintlify documentation validator."""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
VALIDATOR = ROOT / "scripts" / "validate-mintlify-docs.py"

SPEC = importlib.util.spec_from_file_location("validate_mintlify_docs", VALIDATOR)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"could not load {VALIDATOR}")
VALIDATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VALIDATE)


class MintlifyHeadingSlugTests(unittest.TestCase):
    def test_percent_encodes_slash_like_mintlify(self) -> None:
        self.assertEqual(VALIDATE.slugify("capabilities/get"), "capabilities%2Fget")

    def test_preserves_underscores_and_removes_apostrophes(self) -> None:
        self.assertEqual(VALIDATE.slugify("What's _new_?"), "whats-_new_%3F")

    def test_duplicate_headings_start_at_suffix_two(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "duplicates.mdx"
            path.write_text("## Repeat\n## Repeat\n## Repeat\n", encoding="utf-8")
            self.assertEqual(
                VALIDATE.heading_slugs(path),
                {"repeat", "repeat-2", "repeat-3"},
            )


if __name__ == "__main__":
    unittest.main()
