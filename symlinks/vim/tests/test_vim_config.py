import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
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

    def test_neovim_startup_and_terminal_navigation(self):
        executable = shutil.which("nvim")
        if executable is None:
            self.skipTest("nvim is not available")
        with tempfile.TemporaryDirectory(prefix=".nvim-startup-test-", dir=Path.cwd()) as directory:
            fixture = Path(directory)
            home = fixture / "home with spaces"
            home.mkdir()
            env = os.environ.copy()
            env["HOME"] = str(home)
            for key in ("XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "TMPDIR"):
                path = fixture / key.lower()
                path.mkdir()
                env[key] = str(path)
            for key in ("VIMINIT", "EXINIT", "NVIM_APPNAME"):
                env.pop(key, None)
            env["NVIM_LOG_FILE"] = str(fixture / "nvim.log")
            env["DOTFILES_NVIM_STARTUP_TEST"] = str(CONFIG / "tests/test_startup.lua")
            (home / ".vim").symlink_to(CONFIG, target_is_directory=True)
            (Path(env["XDG_CONFIG_HOME"]) / "nvim").symlink_to(CONFIG, target_is_directory=True)
            # Exercise the actual entry points without installing or loading plugins.
            stub = 'lua package.preload["lazy-bootstrap"] = function() vim.g.test_bootstrap_loaded = true; return {} end'
            result = subprocess.run(
                [executable, "--headless", "-i", "NONE", "-n", "--noplugin",
                 "--cmd", stub, "-c", "lua dofile(vim.env.DOTFILES_NVIM_STARTUP_TEST)"],
                cwd=fixture, env=env, text=True, capture_output=True, timeout=30,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertFalse((Path(env["XDG_DATA_HOME"]) / "nvim/lazy").exists())


if __name__ == "__main__":
    unittest.main()
