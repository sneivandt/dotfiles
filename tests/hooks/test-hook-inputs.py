#!/usr/bin/env python3
"""Staged-input, allowlist, and build-trigger regression tests for Git hooks.

Usage: python3 -B tests/hooks/test-hook-inputs.py [test_method ...]
"""

from __future__ import annotations

from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))

from hook_inputs_tests.test_allowlists import AllowlistTests
from hook_inputs_tests.test_build_triggers import BuildTriggerTests
from hook_inputs_tests.test_staged_guards import StagedGuardTests

TEST_CLASSES = (StagedGuardTests, BuildTriggerTests, AllowlistTests)


def build_suite(selected: list[str]) -> unittest.TestSuite:
    loader = unittest.TestLoader()
    if not selected:
        suite = unittest.TestSuite()
        for cls in TEST_CLASSES:
            suite.addTests(loader.loadTestsFromTestCase(cls))
        return suite

    method_map: dict[str, type[unittest.TestCase]] = {}
    for cls in TEST_CLASSES:
        for name in loader.getTestCaseNames(cls):
            method_map[name] = cls

    suite = unittest.TestSuite()
    for name in selected:
        cls = method_map.get(name)
        if cls is None:
            raise SystemExit(f"ERROR: unknown hook-input test: {name}")
        suite.addTest(cls(name))
    return suite


def main(argv: list[str]) -> int:
    suite = build_suite(argv)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
