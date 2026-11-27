#!/usr/bin/env python3
"""Installed-home Bash startup, prompt, and history smoke tests.

Usage: python3 -B tests/integration/test_bash.py [--strict] [startup] [prompt]
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


def home_dir() -> Path:
    value = os.environ.get("HOME")
    return Path(value) if value else Path.home()


class BashTests(unittest.TestCase):
    def setUp(self) -> None:
        if shutil.which("bash") is None:
            self.skipTest("bash not installed")

    def test_startup(self) -> None:
        home = home_dir()
        for rel in (
            ".bashrc",
            ".bash_profile",
            ".config/bash/prompt.bash",
            ".config/shell/interactive.sh",
            ".config/shell/aliases.sh",
            ".config/shell/profile.sh",
        ):
            self.assertTrue((home / rel).is_file(), f"missing installed bash file: {home / rel}")

        env = os.environ.copy()
        env.pop("BASHRC_LOADED", None)
        env.pop("BASH_PROFILE_LOADED", None)
        result = subprocess.run(
            [
                "bash",
                "-l",
                "-i",
                "-c",
                'printf "LOADED=%s:%s\\n" "${BASH_PROFILE_LOADED:-}" "${BASHRC_LOADED:-}"\n'
                'printf "HIST=%s:%s\\n" "${HISTCONTROL:-}" "${HISTSIZE:-}"\n'
                "shopt -q histappend || exit 1\n"
                "declare -F __bash_history_append >/dev/null || exit 1\n"
                "declare -F __bash_git_prompt >/dev/null || exit 1\n"
                "! declare -F _dotfiles_init_history >/dev/null || exit 1\n"
                "alias ll >/dev/null || exit 1\n",
            ],
            env=env,
            capture_output=True,
            text=True,
            check=False,
            timeout=20,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("LOADED=1:1", result.stdout)
        self.assertIn("HIST=ignoreboth:100000", result.stdout)

    def test_prompt(self) -> None:
        if shutil.which("git") is None:
            self.skipTest("git not installed")

        env = os.environ.copy()
        env.pop("BASHRC_LOADED", None)
        env.pop("BASH_PROFILE_LOADED", None)
        with tempfile.TemporaryDirectory() as repo_str:
            repo = Path(repo_str)
            subprocess.run(
                ["git", "-c", "init.templateDir=", "init", "-q", "-b", "main", str(repo)],
                check=True,
                capture_output=True,
                text=True,
            )
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(repo),
                    "-c",
                    "user.name=CI Test",
                    "-c",
                    "user.email=ci@test.local",
                    "commit",
                    "--allow-empty",
                    "-q",
                    "-m",
                    "Initial commit",
                ],
                check=True,
                capture_output=True,
                text=True,
            )
            result = subprocess.run(
                [
                    "bash",
                    "-i",
                    "-c",
                    'printf "BRANCH=<%s>\\n" "$(__bash_git_prompt)"\n'
                    'printf "PS1=%s\\n" "$PS1"\n',
                ],
                cwd=repo,
                env=env,
                capture_output=True,
                text=True,
                check=False,
                timeout=20,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("BRANCH=< main>", result.stdout)
            self.assertIn("$(__bash_git_prompt)", result.stdout)


SELECTORS = {
    "startup": "test_startup",
    "prompt": "test_prompt",
}


def main(argv: list[str]) -> int:
    strict, remaining = pop_strict_flag(argv)
    selected = remaining or ["startup", "prompt"]
    suite = unittest.TestSuite()
    for name in selected:
        method = SELECTORS.get(name)
        if method is None:
            print(f"ERROR: unknown bash test: {name}", file=sys.stderr)
            return 2
        suite.addTest(BashTests(method))
    return run_suite(suite, strict=strict)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
