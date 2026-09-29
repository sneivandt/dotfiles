import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import unittest
import uuid


CONFIG = Path(__file__).resolve().parents[1]


class VimConfigTests(unittest.TestCase):
    def test_bootstrap_matches_lockfile(self):
        bootstrap = (CONFIG / "lua/lazy-bootstrap.lua").read_text()
        match = re.search(r'local lazy_commit = "([0-9a-f]{40})"', bootstrap)
        self.assertIsNotNone(match)
        lock = json.loads((CONFIG / "lazy-lock.json").read_text())
        self.assertEqual(match[1], lock["lazy.nvim"]["commit"])

    def check_filetype_options(self, editor):
        executable = shutil.which(editor)
        if executable is None:
            self.skipTest(f"{editor} is not available")
        fixture = Path.cwd() / (".vim-config-test-" + uuid.uuid4().hex)
        fixture.mkdir()
        try:
            env = os.environ.copy()
            for key in ("HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "TMPDIR"):
                directory = fixture / key.lower()
                directory.mkdir()
                env[key] = str(directory)
            env.pop("VIMINIT", None)
            env.pop("EXINIT", None)
            env["DOTFILES_VIM_CONFIG"] = str(CONFIG / "vimrc")
            command = [executable, "-u", "NONE", "-i", "NONE", "-n", "--noplugin"]
            command += ["--headless"] if editor == "nvim" else ["-N", "-es", "-V1"]
            command += ["-S", str(CONFIG / "tests/test_filetype_options.vim")]
            result = subprocess.run(
                command, env=env, text=True, capture_output=True, timeout=30
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        finally:
            shutil.rmtree(fixture)

    def test_vim_filetype_options(self):
        self.check_filetype_options("vim")

    def test_neovim_filetype_options(self):
        self.check_filetype_options("nvim")


if __name__ == "__main__":
    unittest.main()
