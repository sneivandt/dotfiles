#!/usr/bin/env python3
"""Installed-home Vim startup smoke tests.

Usage: python3 -B tests/integration/test_vim.py [--strict] [opens]
"""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys
import unittest

REPO_ROOT = Path(os.environ.get("DIR") or Path(__file__).resolve().parents[2])
sys.path.insert(0, str(REPO_ROOT / "tests"))

from runner import pop_strict_flag, run_suite


class VimTests(unittest.TestCase):
    def test_opens(self) -> None:
        if shutil.which("vim") is None:
            self.skipTest("vim not installed")

        version = subprocess.run(["vim", "--version"], capture_output=True, check=False)
        self.assertEqual(version.returncode, 0, "vim --version failed")

        vimrc = Path(os.environ.get("HOME") or Path.home()) / ".vim" / "vimrc"
        self.assertTrue(vimrc.is_file(), f"vimrc not installed: {vimrc}")

        try:
            run = subprocess.run(
                ["vim", "-E", "-s", "-c", "quit"],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=5,
                check=False,
            )
        except subprocess.TimeoutExpired:
            self.fail("vim timed out loading vimrc")
        self.assertEqual(run.returncode, 0, "vim failed to load vimrc")


SELECTORS = {
    "opens": "test_opens",
}


def main(argv: list[str]) -> int:
    strict, remaining = pop_strict_flag(argv)
    selected = remaining or ["opens"]
    suite = unittest.TestSuite()
    for name in selected:
        method = SELECTORS.get(name)
        if method is None:
            print(f"ERROR: unknown vim test: {name}", file=sys.stderr)
            return 2
        suite.addTest(VimTests(method))
    return run_suite(suite, strict=strict)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
