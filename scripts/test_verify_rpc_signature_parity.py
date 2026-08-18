#!/usr/bin/env python3
"""Focused policy tests for the RPC signature-parity ratchet."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("verify_rpc_signature_parity.py")
SPEC = importlib.util.spec_from_file_location("verify_rpc_signature_parity", SCRIPT)
assert SPEC and SPEC.loader
parity = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = parity
SPEC.loader.exec_module(parity)


class BaselineTrainExpiryTests(unittest.TestCase):
    def test_0_8_24_train_keeps_exact_reviewed_baseline(self) -> None:
        self.assertIsNone(
            parity.baseline_train_expiry_failure("0.8.24", "0.8.25", 239)
        )

    def test_0_8_25_train_blocks_until_all_wrappers_migrate(self) -> None:
        failure = parity.baseline_train_expiry_failure("0.8.25", "0.8.25", 239)
        self.assertIsNotNone(failure)
        assert failure is not None
        self.assertIn("migrate all 239", failure)

    def test_0_8_25_train_passes_after_baseline_is_empty(self) -> None:
        self.assertIsNone(
            parity.baseline_train_expiry_failure("0.8.25", "0.8.25", 0)
        )

    def test_current_policy_marker_names_the_0_8_24_train(self) -> None:
        self.assertEqual(parity.BASELINE_HAND_ROLLED_CURRENT_TRAIN, "0.8.24")
        self.assertEqual(parity.BASELINE_HAND_ROLLED_EXPIRES_AT_TRAIN, "0.8.25")


if __name__ == "__main__":
    unittest.main()
