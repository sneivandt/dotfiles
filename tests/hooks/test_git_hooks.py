#!/usr/bin/env python3
"""Disposable-repository pre-commit/pre-push hook behavior and sensitive-pattern tests.

Usage: python3 -B tests/hooks/test_git_hooks.py [--strict]
"""

from __future__ import annotations

from pathlib import Path
import sys

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))
sys.path.insert(0, str(SCRIPT_DIR.parent))

from git_hook_tests.test_hook_behavior import HookBehaviorTests
from git_hook_tests.test_sensitive_scan import SensitiveScanTests
from runner import pop_strict_flag, run_test_cases


def main(argv: list[str] | None = None) -> int:
    strict, remaining = pop_strict_flag(list(sys.argv[1:] if argv is None else argv))
    if remaining:
        print(f"ERROR: unexpected arguments: {' '.join(remaining)}", file=sys.stderr)
        return 2
    return run_test_cases(HookBehaviorTests, SensitiveScanTests, strict=strict)


if __name__ == "__main__":
    raise SystemExit(main())
