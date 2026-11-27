#!/usr/bin/env python3
"""Installed-home Zsh completion and history widget smoke tests.

Usage: python3 -B tests/integration/test_zsh.py [--strict] [completion] [history]
"""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

REPO_ROOT = Path(os.environ.get("DIR") or Path(__file__).resolve().parents[2])
sys.path.insert(0, str(REPO_ROOT / "tests"))

from runner import pop_strict_flag, run_suite


class ZshTests(unittest.TestCase):
    def test_completion(self) -> None:
        if shutil.which("zsh") is None:
            self.skipTest("zsh not installed")

        completion = REPO_ROOT / "symlinks" / "config" / "zsh" / "completions" / "_dotfiles"
        self.assertTrue(completion.is_file(), f"completion file missing: {completion}")

        # Runtime completion resolves `dotfiles` when the registration script loads.
        env = os.environ.copy()
        env["PATH"] = f"{REPO_ROOT / 'bin'}{os.pathsep}{env.get('PATH', '')}"
        env["COMPLETION_FILE"] = str(completion)

        load = subprocess.run(
            ["zsh", "-c", 'autoload -Uz compinit; compinit -u; source "$COMPLETION_FILE"'],
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(load.returncode, 0, f"completion failed to load:\n{load.stderr}")

        define = subprocess.run(
            [
                "zsh",
                "-c",
                'autoload -Uz compinit; compinit -u; source "$COMPLETION_FILE" '
                "&& typeset -f _clap_dynamic_completer_dotfiles >/dev/null",
            ],
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(
            define.returncode, 0, f"runtime dotfiles completer not defined:\n{define.stderr}"
        )

    def test_history(self) -> None:
        if shutil.which("zsh") is None:
            self.skipTest("zsh not installed")
        if shutil.which("fzf") is None:
            self.skipTest("fzf not installed")

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
            self.assertEqual(
                result.returncode,
                0,
                "Ctrl+R did not bind the standard non-executing fzf history widget",
            )


SELECTORS = {
    "completion": "test_completion",
    "history": "test_history",
}


def main(argv: list[str]) -> int:
    strict, remaining = pop_strict_flag(argv)
    selected = remaining or ["completion", "history"]
    suite = unittest.TestSuite()
    for name in selected:
        method = SELECTORS.get(name)
        if method is None:
            print(f"ERROR: unknown zsh test: {name}", file=sys.stderr)
            return 2
        suite.addTest(ZshTests(method))
    return run_suite(suite, strict=strict)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
