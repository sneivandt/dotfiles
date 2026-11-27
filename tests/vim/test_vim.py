#!/usr/bin/env python3
"""Vim isolated configuration and startup tests.

Usage: python3 -B tests/vim/test_vim.py [opens] [configuration]
"""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys
import unittest

REPO_ROOT = Path(os.environ.get("DIR") or Path(__file__).resolve().parents[2])


class VimTests(unittest.TestCase):
    def test_configuration(self) -> None:
        result = subprocess.run(
            [
                sys.executable,
                "-B",
                "-m",
                "unittest",
                str(REPO_ROOT / "tests" / "vim" / "test_vim_config.py"),
            ],
            check=False,
        )
        self.assertEqual(result.returncode, 0, "isolated Vim configuration tests failed")

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
    "configuration": "test_configuration",
}


def main(argv: list[str]) -> int:
    selected = argv or ["opens", "configuration"]
    suite = unittest.TestSuite()
    for name in selected:
        method = SELECTORS.get(name)
        if method is None:
            print(f"ERROR: unknown vim test: {name}", file=sys.stderr)
            return 2
        suite.addTest(VimTests(method))
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
