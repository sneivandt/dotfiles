#!/usr/bin/env python3
"""Shared unittest runner enforcing strict skip handling when requested.

Usage:
  python3 -B tests/runner.py [--strict] <directory> [--pattern PATTERN]
"""

from __future__ import annotations

import argparse
import os
import sys
from typing import TYPE_CHECKING
import unittest

if TYPE_CHECKING:
    from pathlib import Path


def is_strict(explicit: bool | None = None) -> bool:
    """Return whether skipped tests should fail the run."""
    if explicit is not None:
        return explicit
    return os.environ.get("DOTFILES_CHECK_STRICT") == "1"


def pop_strict_flag(argv: list[str]) -> tuple[bool | None, list[str]]:
    """Extract --strict from argv while preserving all other arguments."""
    strict: bool | None = None
    remaining: list[str] = []
    for arg in argv:
        if arg == "--strict":
            strict = True
        else:
            remaining.append(arg)
    return strict, remaining


def run_suite(
    suite: unittest.TestSuite,
    *,
    strict: bool | None = None,
    verbosity: int = 2,
) -> int:
    """Run a unittest suite and fail on skipped tests when strict mode is active."""
    result = unittest.TextTestRunner(verbosity=verbosity).run(suite)
    if not result.wasSuccessful():
        return 1
    if is_strict(strict) and result.skipped:
        print(
            f"\nERROR: {len(result.skipped)} test(s) skipped in strict mode:",
            file=sys.stderr,
        )
        for test, reason in result.skipped:
            print(f"  - {test}: {reason}", file=sys.stderr)
        return 1
    return 0


def run_test_cases(
    *cases: type[unittest.TestCase],
    strict: bool | None = None,
    verbosity: int = 2,
) -> int:
    """Load and run one or more unittest.TestCase subclasses."""
    loader = unittest.TestLoader()
    suite = unittest.TestSuite()
    for case in cases:
        suite.addTests(loader.loadTestsFromTestCase(case))
    return run_suite(suite, strict=strict, verbosity=verbosity)


def discover_and_run(
    start_dir: str | Path,
    pattern: str = "test_*.py",
    *,
    strict: bool | None = None,
    verbosity: int = 2,
) -> int:
    """Discover tests under start_dir and run them with strict skip handling."""
    loader = unittest.TestLoader()
    suite = loader.discover(start_dir=str(start_dir), pattern=pattern)
    return run_suite(suite, strict=strict, verbosity=verbosity)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--strict",
        action="store_true",
        default=None,
        help="Fail if any test is skipped",
    )
    parser.add_argument("--pattern", default="test_*.py", help="Discovery filename pattern")
    parser.add_argument("directory", help="Directory containing unittest modules")
    args = parser.parse_args(argv)
    return discover_and_run(args.directory, args.pattern, strict=args.strict)


if __name__ == "__main__":
    raise SystemExit(main())
