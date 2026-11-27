#!/usr/bin/env python3
"""Neovim isolated configuration, startup, and plugin tests.

Usage: python3 -B tests/vim/test_nvim.py [opens] [plugins] [configuration]
"""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys
import unittest

REPO_ROOT = Path(os.environ.get("DIR") or Path(__file__).resolve().parents[2])
LOCKFILE = REPO_ROOT / "symlinks" / "vim" / "lazy-lock.json"


class NvimTests(unittest.TestCase):
    def setUp(self) -> None:
        self._lock_before = LOCKFILE.read_bytes()

    def tearDown(self) -> None:
        self.assertEqual(
            LOCKFILE.read_bytes(),
            self._lock_before,
            "Neovim tests changed the tracked plugin lockfile",
        )

    def _run_nvim(
        self,
        args: list[str],
        *,
        timeout: int,
        env: dict[str, str] | None = None,
        capture: bool = True,
    ) -> int:
        try:
            result = subprocess.run(
                ["nvim", *args],
                env=env,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL if capture else None,
                stderr=subprocess.DEVNULL if capture else None,
                timeout=timeout,
                check=False,
            )
        except subprocess.TimeoutExpired:
            self.fail(f"nvim timed out after {timeout}s: {' '.join(args)}")
        return result.returncode

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
        self.assertEqual(result.returncode, 0, "isolated Neovim configuration tests failed")

    def test_opens(self) -> None:
        if shutil.which("nvim") is None:
            self.skipTest("nvim not installed")

        env = os.environ.copy()
        env["DIR"] = str(REPO_ROOT)
        config_check = self._run_nvim(
            [
                "--headless",
                "-u",
                "NONE",
                "-i",
                "NONE",
                "-n",
                "-c",
                'lua local ok, err = pcall(dofile, vim.env.DIR .. "/tests/vim/test-nvim-config.lua"); '
                'if not ok then vim.api.nvim_err_writeln(tostring(err)); vim.cmd("cquit") end',
                "-c",
                "qa!",
            ],
            timeout=10,
            env=env,
            capture=False,
        )
        self.assertEqual(config_check, 0, "test-nvim-config.lua failed")
        self.assertEqual(self._run_nvim(["--version"], timeout=10), 0, "nvim --version failed")

        home = Path(os.environ.get("HOME") or Path.home())
        nvim_config = home / ".config" / "nvim"
        self.assertTrue(nvim_config.is_dir(), f"nvim config not installed: {nvim_config}")
        self.assertEqual(
            self._run_nvim(["--headless", "-c", ":qa!"], timeout=120),
            0,
            "nvim failed to load config",
        )

    def test_plugins(self) -> None:
        if shutil.which("nvim") is None:
            self.skipTest("nvim not installed")
        home = Path(os.environ.get("HOME") or Path.home())
        nvimrc = home / ".config" / "nvim" / "nvimrc"
        self.assertTrue(nvimrc.is_file(), f"nvimrc not installed: {nvimrc}")

        lazy_dir = home / ".local" / "share" / "nvim" / "lazy"
        self.assertEqual(
            self._run_nvim(["--headless", "+Lazy! restore", "+qa!"], timeout=180),
            0,
            "lazy.nvim failed to restore locked plugins",
        )
        self.assertTrue(
            (lazy_dir / "lazy.nvim").is_dir(),
            f"lazy.nvim not bootstrapped: {lazy_dir / 'lazy.nvim'}",
        )

        env = os.environ.copy()
        env["DIR"] = str(REPO_ROOT)
        lock_check = self._run_nvim(
            [
                "--headless",
                "-c",
                'lua local ok, err = pcall(dofile, vim.env.DIR .. "/tests/vim/test-nvim-lock.lua"); '
                'if not ok then vim.api.nvim_err_writeln(tostring(err)); vim.cmd("cquit") end',
                "-c",
                "qa!",
            ],
            timeout=30,
            env=env,
            capture=False,
        )
        self.assertEqual(lock_check, 0, "test-nvim-lock.lua failed")
        self.assertEqual(
            self._run_nvim(["--headless", "+qa!"], timeout=30),
            0,
            "nvim plugin load failed",
        )


SELECTORS = {
    "opens": "test_opens",
    "plugins": "test_plugins",
    "configuration": "test_configuration",
}


def main(argv: list[str]) -> int:
    selected = argv or ["opens", "plugins", "configuration"]
    suite = unittest.TestSuite()
    for name in selected:
        method = SELECTORS.get(name)
        if method is None:
            print(f"ERROR: unknown nvim test: {name}", file=sys.stderr)
            return 2
        suite.addTest(NvimTests(method))
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
