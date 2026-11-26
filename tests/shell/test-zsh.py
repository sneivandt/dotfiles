#!/usr/bin/env python3
"""Zsh configuration, completion, and history widget tests.

Usage: python3 -B tests/shell/test-zsh.py [completion] [history] [configuration]
"""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

REPO_ROOT = Path(os.environ.get("DIR") or Path(__file__).resolve().parents[2])


def fail(message: str) -> None:
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def test_configuration() -> None:
    print("=== Testing isolated shell configuration")
    result = subprocess.run(
        [sys.executable, "-B", str(REPO_ROOT / "tests" / "shell" / "test-shell-config.py")],
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(result.returncode)


def test_completion() -> None:
    if shutil.which("zsh") is None:
        print("Skipping: zsh not installed")
        return
    print("=== Validating zsh completion")

    completion = REPO_ROOT / "symlinks" / "config" / "zsh" / "completions" / "_dotfiles"
    if not completion.is_file():
        fail(f"completion file missing: {completion}")

    # Runtime completion resolves `dotfiles` when the registration script loads.
    env = os.environ.copy()
    env["PATH"] = f"{REPO_ROOT / 'bin'}{os.pathsep}{env.get('PATH', '')}"
    env["COMPLETION_FILE"] = str(completion)

    load = subprocess.run(
        ["zsh", "-c", 'autoload -Uz compinit; compinit -u; source "$COMPLETION_FILE"'],
        env=env,
        capture_output=True,
        check=False,
    )
    if load.returncode != 0:
        fail("completion failed to load")
    print("PASS: Completion file loads OK")

    define = subprocess.run(
        [
            "zsh",
            "-c",
            'autoload -Uz compinit; compinit -u; source "$COMPLETION_FILE" '
            "&& typeset -f _clap_dynamic_completer_dotfiles >/dev/null",
        ],
        env=env,
        capture_output=True,
        check=False,
    )
    if define.returncode != 0:
        fail("runtime dotfiles completer not defined")
    print("PASS: Completion functions defined")


def test_history() -> None:
    if shutil.which("zsh") is None:
        print("Skipping: zsh not installed")
        return
    if shutil.which("fzf") is None:
        print("Skipping: fzf not installed")
        return
    print("=== Testing zsh history selection")

    with tempfile.TemporaryDirectory() as fixture_str:
        fixture = Path(fixture_str)
        (fixture / ".config").mkdir()
        os.symlink(REPO_ROOT / "symlinks" / "config" / "shell", fixture / ".config" / "shell")
        env = os.environ.copy()
        env["HOME"] = str(fixture)
        env["XDG_CACHE_HOME"] = str(fixture / ".cache")
        env["DIR"] = str(REPO_ROOT)

        result = subprocess.run(
            [
                "zsh",
                "-d",
                "-f",
                "-i",
                "-c",
                'source "$DIR/symlinks/zshrc"\n'
                '[[ "$(bindkey -M emacs "^R")" == "\\"^R\\" fzf-history-widget" ]] || exit 1\n'
                "(( ! $+widgets[fzf-history-widget-accept] )) || exit 1\n",
            ],
            env=env,
            check=False,
        )
        if result.returncode != 0:
            fail("Ctrl+R did not bind the standard non-executing fzf history widget")
        print("PASS: Ctrl+R uses the standard non-executing fzf history widget")


TESTS = {
    "completion": test_completion,
    "history": test_history,
    "configuration": test_configuration,
}


def main(argv: list[str]) -> int:
    selected = argv or ["completion", "history", "configuration"]
    for name in selected:
        fn = TESTS.get(name)
        if fn is None:
            fail(f"unknown zsh test: {name}")
        fn()
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
