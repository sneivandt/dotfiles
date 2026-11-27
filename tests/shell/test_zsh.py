#!/usr/bin/env python3
"""Zsh configuration, completion, and history widget tests.

Usage: python3 -B tests/shell/test_zsh.py [completion] [history] [configuration]
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


class ZshTests(unittest.TestCase):
    def test_configuration(self) -> None:
        result = subprocess.run(
            [sys.executable, "-B", str(REPO_ROOT / "tests" / "shell" / "test_shell_config.py")],
            check=False,
        )
        self.assertEqual(result.returncode, 0, "isolated shell configuration suite failed")

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
    "configuration": "test_configuration",
}


def main(argv: list[str]) -> int:
    selected = argv or ["completion", "history", "configuration"]
    suite = unittest.TestSuite()
    for name in selected:
        method = SELECTORS.get(name)
        if method is None:
            print(f"ERROR: unknown zsh test: {name}", file=sys.stderr)
            return 2
        suite.addTest(ZshTests(method))
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
