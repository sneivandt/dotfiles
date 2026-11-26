#!/usr/bin/env python3
"""Full pre-commit/pre-push hook integration test runner.

Runs disposable-repository hook behavior and sensitive-pattern suites,
followed by staged hook-input regressions and maintenance tests.
"""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys
import unittest

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

from git_hook_tests.test_hook_behavior import HookBehaviorTests
from git_hook_tests.test_sensitive_scan import SensitiveScanTests


def main() -> int:
    loader = unittest.TestLoader()
    suite = unittest.TestSuite()
    suite.addTests(loader.loadTestsFromTestCase(HookBehaviorTests))
    suite.addTests(loader.loadTestsFromTestCase(SensitiveScanTests))
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    if not result.wasSuccessful():
        return 1

    for script in ("test-hook-inputs.py", "test-maintenance.py"):
        print(f"\n=== Running {script}")
        proc = subprocess.run([sys.executable, "-B", str(SCRIPT_DIR / script)], check=False)
        if proc.returncode != 0:
            return proc.returncode

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
