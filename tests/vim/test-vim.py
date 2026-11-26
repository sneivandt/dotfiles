#!/usr/bin/env python3
"""Vim isolated configuration and startup tests.

Usage: python3 -B tests/vim/test-vim.py [opens] [configuration]
"""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys

REPO_ROOT = Path(os.environ.get("DIR") or Path(__file__).resolve().parents[2])


def fail(message: str) -> None:
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def test_configuration() -> None:
    print("=== Testing isolated Vim configuration")
    result = subprocess.run(
        [sys.executable, "-B", "-m", "unittest", "discover", "-s", str(REPO_ROOT / "tests" / "vim"), "-p", "test_*.py"],
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(result.returncode)


def test_opens() -> None:
    if shutil.which("vim") is None:
        print("Skipping: vim not installed")
        return
    print("=== Testing vim startup")

    version = subprocess.run(["vim", "--version"], capture_output=True, check=False)
    if version.returncode != 0:
        fail("vim --version failed")
    print("PASS: Vim binary OK")

    vimrc = Path(os.environ.get("HOME") or Path.home()) / ".vim" / "vimrc"
    if not vimrc.is_file():
        fail(f"vimrc not installed: {vimrc}")

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
        fail("vim timed out loading vimrc")
    if run.returncode != 0:
        fail("vim failed to load vimrc")
    print("PASS: Vim loads custom vimrc")


TESTS = {
    "opens": test_opens,
    "configuration": test_configuration,
}


def main(argv: list[str]) -> int:
    selected = argv or ["opens", "configuration"]
    for name in selected:
        fn = TESTS.get(name)
        if fn is None:
            fail(f"unknown vim test: {name}")
        fn()
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
