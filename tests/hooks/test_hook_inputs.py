#!/usr/bin/env python3
"""Staged-input, allowlist, and build-trigger regression tests for Git hooks.

Usage:
  python3 -B tests/hooks/test_hook_inputs.py [--strict] [--skip-build-triggers] [test_method ...]
"""

from __future__ import annotations

from pathlib import Path
import sys
import unittest

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))
sys.path.insert(0, str(SCRIPT_DIR.parent))

from hook_inputs_tests.test_allowlists import AllowlistTests
from hook_inputs_tests.test_build_triggers import BuildTriggerTests
from hook_inputs_tests.test_staged_guards import StagedGuardTests
from runner import pop_strict_flag, run_suite

TEST_CLASSES = (StagedGuardTests, BuildTriggerTests, AllowlistTests)


def build_suite(selected: list[str], *, skip_build_triggers: bool = False) -> unittest.TestSuite:
    loader = unittest.TestLoader()
    if not selected:
        suite = unittest.TestSuite()
        for cls in TEST_CLASSES:
            if skip_build_triggers and cls is BuildTriggerTests:
                continue
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
    strict, remaining = pop_strict_flag(argv)
    skip_build_triggers = False
    selected: list[str] = []
    for arg in remaining:
        if arg == "--skip-build-triggers":
            skip_build_triggers = True
        else:
            selected.append(arg)
    suite = build_suite(selected, skip_build_triggers=skip_build_triggers)
    return run_suite(suite, strict=strict)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
