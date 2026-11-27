#!/usr/bin/env python3
"""Tests for the POSIX dotfiles.sh wrapper.

Usage:
  python3 -B tests/wrapper/test_shell_wrapper_linux.py                 # run full Linux wrapper suite + parity
  python3 -B tests/wrapper/test_shell_wrapper_linux.py --isolated      # skip real-binary/cargo-build cases
  python3 -B tests/wrapper/test_shell_wrapper_linux.py [test_name...]  # run specific test method(s)
"""

from __future__ import annotations

import os
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))

from linux.test_args_and_build import WrapperArgsAndBuildTests
from linux.test_attestation import WrapperAttestationTests
from linux.test_bootstrap import WrapperBootstrapTests
from test_parity import WrapperParityTests

__all__ = [
    "WrapperArgsAndBuildTests",
    "WrapperAttestationTests",
    "WrapperBootstrapTests",
    "WrapperParityTests",
]

_TEST_CLASSES = (
    WrapperArgsAndBuildTests,
    WrapperBootstrapTests,
    WrapperAttestationTests,
    WrapperParityTests,
)


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    isolated = False
    selectors: list[str] = []
    for arg in args:
        if arg == "--isolated":
            isolated = True
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
        for cls in _TEST_CLASSES:
            suite.addTests(loader.loadTestsFromTestCase(cls))

    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
