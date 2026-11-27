#!/usr/bin/env python3
"""Tests for the POSIX dotfiles.sh wrapper.

Usage:
  python3 -B tests/wrapper/test_shell_wrapper_linux.py [--strict]                 # run full Linux wrapper suite
  python3 -B tests/wrapper/test_shell_wrapper_linux.py [--strict] --isolated      # run isolated fixture cases only
  python3 -B tests/wrapper/test_shell_wrapper_linux.py [--strict] --prebuilt      # exclude cargo --build case (CI prebuilt binary)
  python3 -B tests/wrapper/test_shell_wrapper_linux.py [--strict] [test_name...]  # run specific test method(s)
"""

from __future__ import annotations

import os
from pathlib import Path
import sys
import unittest

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))
sys.path.insert(0, str(SCRIPT_DIR.parent))

from linux.test_args_and_build import WrapperArgsAndBuildTests
from linux.test_attestation import WrapperAttestationTests
from linux.test_bootstrap import WrapperBootstrapTests
from runner import pop_strict_flag, run_suite

__all__ = [
    "WrapperArgsAndBuildTests",
    "WrapperAttestationTests",
    "WrapperBootstrapTests",
]

_TEST_CLASSES = (
    WrapperArgsAndBuildTests,
    WrapperBootstrapTests,
    WrapperAttestationTests,
)
_BINARY_TESTS = frozenset(
    {
        "test_wrapper_build_mode",
        "test_wrapper_uses_local_binary",
        "test_wrapper_forwarded_args",
    }
)
_BUILD_ONLY_TESTS = frozenset({"test_wrapper_build_mode"})


def main(argv: list[str] | None = None) -> int:
    strict, remaining = pop_strict_flag(list(sys.argv[1:] if argv is None else argv))
    isolated = False
    prebuilt = False
    selectors: list[str] = []
    for arg in remaining:
        if arg == "--isolated":
            isolated = True
        elif arg == "--prebuilt":
            prebuilt = True
        else:
            selectors.append(arg)

    if isolated:
        os.environ["DOTFILES_WRAPPER_ISOLATED"] = "1"

    loader = unittest.TestLoader()
    suite = unittest.TestSuite()
    if selectors:
        for name in selectors:
            matches = [cls(name) for cls in _TEST_CLASSES if hasattr(cls, name)]
            if not matches:
                print(f"ERROR: unknown wrapper test: {name}", file=sys.stderr)
                return 2
            suite.addTests(matches)
    else:
        excluded: set[str] = set()
        if isolated:
            excluded.update(_BINARY_TESTS)
        if prebuilt:
            excluded.update(_BUILD_ONLY_TESTS)
        for cls in _TEST_CLASSES:
            for name in loader.getTestCaseNames(cls):
                if name in excluded:
                    continue
                suite.addTest(cls(name))

    return run_suite(suite, strict=strict)


if __name__ == "__main__":
    raise SystemExit(main())
