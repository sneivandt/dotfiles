#!/usr/bin/env python3
"""Neovim isolated configuration, startup, and plugin tests.

Usage: python3 -B tests/vim/test-nvim.py [opens] [plugins] [configuration]
"""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys

REPO_ROOT = Path(os.environ.get("DIR") or Path(__file__).resolve().parents[2])
LOCKFILE = REPO_ROOT / "symlinks" / "vim" / "lazy-lock.json"


def fail(message: str) -> None:
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def run_nvim(args: list[str], *, timeout: int, env: dict[str, str] | None = None, capture: bool = True) -> int:
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
        fail(f"nvim timed out after {timeout}s: {' '.join(args)}")
    return result.returncode


def test_configuration() -> None:
    print("=== Testing isolated Neovim configuration")
    result = subprocess.run(
        [sys.executable, "-B", "-m", "unittest", "discover", "-s", str(REPO_ROOT / "tests" / "vim"), "-p", "test_*.py"],
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(result.returncode)


def test_opens() -> None:
    if shutil.which("nvim") is None:
        print("Skipping: nvim not installed")
        return
    print("=== Testing nvim startup")

    env = os.environ.copy()
    env["DIR"] = str(REPO_ROOT)
    config_check = run_nvim(
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
    if config_check != 0:
        fail("test-nvim-config.lua failed")

    if run_nvim(["--version"], timeout=10) != 0:
        fail("nvim --version failed")
    print("PASS: Nvim binary OK")

    home = Path(os.environ.get("HOME") or Path.home())
    nvim_config = home / ".config" / "nvim"
    if not nvim_config.is_dir():
        fail(f"nvim config not installed: {nvim_config}")

    if run_nvim(["--headless", "-c", ":qa!"], timeout=120) != 0:
        fail("nvim failed to load config")
    print("PASS: Nvim loads custom config")


def test_plugins() -> None:
    if shutil.which("nvim") is None:
        print("Skipping: nvim not installed")
        return
    home = Path(os.environ.get("HOME") or Path.home())
    nvimrc = home / ".config" / "nvim" / "nvimrc"
    if not nvimrc.is_file():
        fail(f"nvimrc not installed: {nvimrc}")
    print("=== Testing nvim plugins")

    lazy_dir = home / ".local" / "share" / "nvim" / "lazy"
    if run_nvim(["--headless", "+Lazy! restore", "+qa!"], timeout=180) != 0:
        fail("lazy.nvim failed to restore locked plugins")
    if not (lazy_dir / "lazy.nvim").is_dir():
        fail(f"lazy.nvim not bootstrapped: {lazy_dir / 'lazy.nvim'}")

    env = os.environ.copy()
    env["DIR"] = str(REPO_ROOT)
    lock_check = run_nvim(
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
    if lock_check != 0:
        fail("test-nvim-lock.lua failed")

    plugin_count = sum(1 for entry in lazy_dir.iterdir() if entry.is_dir())
    print(f"PASS: Found {plugin_count} plugin directories")

    if run_nvim(["--headless", "+qa!"], timeout=30) != 0:
        fail("nvim plugin load failed")
    print("PASS: Nvim starts with plugins OK")


TESTS = {
    "opens": test_opens,
    "plugins": test_plugins,
    "configuration": test_configuration,
}


def main(argv: list[str]) -> int:
    selected = argv or ["opens", "plugins", "configuration"]
    lock_before = LOCKFILE.read_bytes()
    for name in selected:
        fn = TESTS.get(name)
        if fn is None:
            fail(f"unknown nvim test: {name}")
        fn()
        if LOCKFILE.read_bytes() != lock_before:
            fail("Neovim tests changed the tracked plugin lockfile")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
