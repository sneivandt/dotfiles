"""Shared isolated fixture harness for managed shell configuration tests."""

import os
from pathlib import Path
import shutil
import subprocess
import unittest
import uuid


ROOT = Path(__file__).resolve().parents[3]


class ShellConfigTestCase(unittest.TestCase):
    def setUp(self):
        self.fixture = ROOT / (".shell-config-test-" + uuid.uuid4().hex)
        self.fixture.mkdir(mode=0o700)
        self.addCleanup(shutil.rmtree, self.fixture)
        self.home = self.fixture / "home with spaces"
        self.home.mkdir(mode=0o700)
        self.env = {
            "HOME": str(self.home),
            "PATH": "/usr/bin:/bin",
            "TERM": "xterm-256color",
            "LC_ALL": "C",
            "SHELL": "/usr/bin/zsh",
            "ROOT": str(ROOT),
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_CONFIG_COUNT": "0",
        }

    def write(self, path, contents, executable=False):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents, encoding="utf-8")
        path.chmod(0o755 if executable else 0o644)
        return path

    def run_shell(self, script, shell="zsh", interactive=False, expected=0):
        args = (["bash", "--noprofile", "--norc", "-ic" if interactive else "-c"]
                if shell == "bash" else ["zsh", "-dfi" if interactive else "-df", "-c"])
        if shell == "zsh" and "ZSH_COMPDUMP" in self.env:
            # Test the real completion initializer, not optional host/vendor
            # completion trees with runner-specific ownership.
            script = 'fpath=(${^fpath}/compinit(N:h))\n' + script
        result = subprocess.run(
            args + [script], cwd=self.fixture, env=self.env,
            text=True, capture_output=True, timeout=20,
        )
        self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
        return result

    def install_profile(self):
        directory = self.home / ".config/shell"
        directory.mkdir(parents=True)
        for name in ("profile.sh", "path.sh", "interactive.sh"):
            (directory / name).symlink_to(ROOT / "symlinks/config/shell" / name)
        self.write(self.home / ".local/bin/nvim", "#!/bin/sh\nexit 0\n", executable=True)

    def prepare_completion(self):
        cache = self.home / ".cache/zsh"
        self.write(cache / "hosts.cache", "typeset -ga hosts=(localhost)\n")
        self.env["ZSH_COMPDUMP"] = str(cache / "zcompdump-fixture")
        # Keep compaudit's trust checks enabled even with a permissive CI umask.
        completion_dir = self.home / ".config/zsh/completions"
        for directory in (completion_dir.parent.parent, completion_dir.parent, completion_dir):
            directory.mkdir(mode=0o700, exist_ok=True)
        self.write(
            completion_dir / "_dotfiles",
            "source <(DOTFILES_COMPLETE=zsh dotfiles)\n",
        )
        binary = self.write(
            self.fixture / "bin/dotfiles",
            "#!/bin/sh\n"
            '[ "$DOTFILES_COMPLETE" = zsh ] || exit 1\n'
            "printf '%s\\n' '_clap_dynamic_completer_dotfiles() { return 0; }' "
            "'compdef _clap_dynamic_completer_dotfiles dotfiles'\n",
            executable=True,
        )
        self.env["PATH"] = str(binary.parent) + ":/usr/bin:/bin"
        return Path(self.env["ZSH_COMPDUMP"])
