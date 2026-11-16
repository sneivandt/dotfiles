from contextlib import contextmanager
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest
import uuid


CONFIG = Path(__file__).resolve().parents[1]

MANAGER = """\
return { setup = function() vim.g.bootstrap_test_ready = true end }
"""


class NeovimBootstrapTests(unittest.TestCase):
    @contextmanager
    def bootstrap_fixture(self, mode=""):
        executable = shutil.which("nvim")
        if executable is None:
            self.skipTest("nvim is not available")
        with tempfile.TemporaryDirectory(prefix=".nvim-bootstrap-test-", dir=Path.cwd()) as directory:
            root = Path(directory)
            env = os.environ.copy()
            for key in ("HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "TMPDIR"):
                path = root / (key.lower() + " with spaces")
                path.mkdir()
                env[key] = str(path)
            for key in ("VIMINIT", "EXINIT", "NVIM_APPNAME"):
                env.pop(key, None)
            env["NVIM_LOG_FILE"] = str(root / "nvim.log")
            env["BOOTSTRAP_FIXTURE"] = str(root)
            env["BOOTSTRAP_MODE"] = mode
            env["BOOTSTRAP_SOURCE"] = str(CONFIG / "lua/lazy-bootstrap.lua")
            env["BOOTSTRAP_MANAGER"] = MANAGER
            env["PATH"] = str(root) + os.pathsep + env["PATH"]
            git = root / "git"
            git.write_text(f"#!{sys.executable}\n" + textwrap.dedent("""\
                import json
                import os
                from pathlib import Path
                import sys
                import time

                root = Path(os.environ["BOOTSTRAP_FIXTURE"])
                mode = os.environ["BOOTSTRAP_MODE"]
                args = sys.argv[1:]
                with (root / "git-calls").open("a") as calls:
                    calls.write(json.dumps(args) + "\\n")
                if args[0] == "clone":
                    path = Path(args[-1])
                    final = Path(os.environ["XDG_DATA_HOME"]) / "nvim/lazy/lazy.nvim"
                    assert path != final, "Clone must not write into the published path"
                    (path / "partial-clone").write_text("interrupted clone")
                    if mode == "existing-during-clone":
                        (final / "lua/lazy/core").mkdir(parents=True)
                        (final / "lua/lazy/init.lua").write_text(os.environ["BOOTSTRAP_MANAGER"])
                        (final / "lua/lazy/core/config.lua").write_text("return {}")
                        (final / "user-revision").write_text("not the bootstrap pin")
                    if mode == "clone-failure":
                        sys.exit("fixture clone failure")
                    if mode == "concurrent":
                        (root / ("clone-" + str(os.getpid()))).touch()
                        deadline = time.monotonic() + 10
                        while len(list(root.glob("clone-*"))) < 2:
                            assert time.monotonic() < deadline, "Second clone never arrived"
                            time.sleep(0.01)
                    (path / "lua/lazy/core").mkdir(parents=True)
                    (path / "lua/lazy/init.lua").write_text(os.environ["BOOTSTRAP_MANAGER"])
                    if mode != "invalid-checkout":
                        (path / "lua/lazy/core/config.lua").write_text("return {}")
                else:
                    assert args[0] == "-C" and args[2] == "checkout", args
                    if mode == "checkout-failure":
                        sys.exit("fixture checkout failure")
                    (Path(args[1]) / "checked-out").write_text(args[3])
                """))
            git.chmod(0o755)
            command = [
                executable, "--headless", "-u", "NONE", "-i", "NONE", "-n", "--noplugin",
                "-c", """lua
local ok, failure = pcall(function()
  local mode = vim.env.BOOTSTRAP_MODE
  if mode == "publish-failure" or mode == "crash-after-lock" or mode == "crash-after-backup" then
    local uv = vim.uv or vim.loop
    local rename = uv.fs_rename
    uv.fs_rename = function(source, target)
      local final = vim.fn.stdpath("data") .. "/lazy/lazy.nvim"
      if mode == "publish-failure" and target == final then
        return nil, "fixture publish failure"
      end
      local result, message, code = rename(source, target)
      if result and ((mode == "crash-after-lock" and target == final .. ".bootstrap-lock")
        or (mode == "crash-after-backup" and source == final)) then
        uv.kill(uv.os_getpid(), 9)
      end
      return result, message, code
    end
  elseif mode == "active-lock" or mode == "unverifiable-lock" or mode == "successor-lock" then
    local wait = vim.wait
    vim.wait = function(_, callback, interval) return wait(50, callback, interval) end
    local uv = vim.uv or vim.loop
    if mode == "unverifiable-lock" then
      uv.kill = function() return nil, "fixture permission denied", "EPERM" end
    elseif mode == "successor-lock" then
      local unlink = uv.fs_unlink
      local replaced = false
      uv.fs_unlink = function(path)
        if not replaced and path:find(".bootstrap-lock/owner-", 1, true) then
          replaced = true
          local lock = vim.fn.fnamemodify(path, ":h")
          assert(unlink(path))
          assert(uv.fs_rmdir(lock))
          assert(uv.fs_mkdir(lock, 448))
          vim.fn.writefile({ "preserve" }, lock .. "/owner-" .. uv.os_getpid() .. "-successor")
          return nil, "fixture owner changed", "ENOENT"
        end
        return unlink(path)
      end
    end
  end
  dofile(vim.env.BOOTSTRAP_SOURCE)
  assert(vim.g.bootstrap_test_ready, "Published manager must load and configure")
end)
if not ok then
  vim.api.nvim_err_writeln(tostring(failure))
  vim.cmd("cquit")
end
""",
                "-c", "qa!",
            ]
            self.root = root
            self.manager = Path(env["XDG_DATA_HOME"]) / "nvim/lazy/lazy.nvim"
            self.command = command
            self.env = env
            yield

    def write_manager(self, path):
        (path / "lua/lazy/core").mkdir(parents=True)
        (path / "lua/lazy/init.lua").write_text(MANAGER)
        (path / "lua/lazy/core/config.lua").write_text("return {}")

    def run_bootstrap(self, error=None):
        result = subprocess.run(
            self.command, cwd=self.root, env=self.env,
            text=True, capture_output=True, timeout=30,
        )
        output = result.stdout + result.stderr
        self.assertEqual(result.returncode, 0 if error is None else 1, output)
        if error is not None:
            self.assertIn(error, output)
        return output

    def git_calls(self):
        path = self.root / "git-calls"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def assert_no_staging(self):
        self.assertEqual(list(self.manager.parent.glob("lazy.nvim.bootstrap-*")), [])

    def write_dead_lock(self):
        exited = subprocess.Popen([sys.executable, "-c", "pass"])
        self.assertEqual(exited.wait(timeout=10), 0)
        lock = self.manager.with_name("lazy.nvim.bootstrap-lock")
        lock.mkdir(parents=True)
        (lock / f"owner-{exited.pid}-fixture").touch()
        return lock

    def test_absent_manager_is_pinned_before_publication(self):
        with self.bootstrap_fixture():
            self.run_bootstrap()
            calls = self.git_calls()
            self.assertEqual(len(calls), 2)
            self.assertEqual(calls[0][:3], ["clone", "--filter=blob:none", "https://github.com/folke/lazy.nvim.git"])
            self.assertNotEqual(calls[0][-1], str(self.manager))
            pin = json.loads((CONFIG / "lazy-lock.json").read_text())["lazy.nvim"]["commit"]
            self.assertEqual(calls[1], ["-C", calls[0][-1], "checkout", pin])
            self.assertEqual((self.manager / "checked-out").read_text(), pin)
            self.assert_no_staging()

    def test_existing_manager_and_symlink_are_not_reset(self):
        for symlink in (False, True):
            with self.subTest(symlink=symlink), self.bootstrap_fixture():
                installed = self.root / "user manager" if symlink else self.manager
                self.write_manager(installed)
                marker = installed / "user-revision"
                marker.write_text("not the bootstrap pin")
                if symlink:
                    self.manager.parent.mkdir(parents=True)
                    self.manager.symlink_to(installed, target_is_directory=True)
                # Git must not even be required for an existing installation.
                (self.root / "git").unlink()
                self.env["PATH"] = str(self.root)
                self.run_bootstrap()
                self.assertEqual(marker.read_text(), "not the bootstrap pin")
                self.assertEqual(self.manager.is_symlink(), symlink)
                self.assertEqual(self.git_calls(), [])

    def test_interrupted_staging_does_not_block_retry(self):
        with self.bootstrap_fixture():
            abandoned = self.manager.with_name("lazy.nvim.bootstrap-abandoned")
            abandoned.mkdir(parents=True)
            (abandoned / "partial").write_text("preserve")
            self.run_bootstrap()
            self.assertTrue((self.manager / "checked-out").is_file())
            self.assertEqual((abandoned / "partial").read_text(), "preserve")
            self.assertEqual(list(self.manager.parent.glob("lazy.nvim.bootstrap-*")), [abandoned])

    def test_manager_installed_during_clone_is_not_reset(self):
        with self.bootstrap_fixture("existing-during-clone"):
            self.run_bootstrap()
            self.assertEqual((self.manager / "user-revision").read_text(), "not the bootstrap pin")
            self.assertFalse((self.manager / "checked-out").exists())
            self.assert_no_staging()

    def test_legacy_partial_directory_is_preserved_and_recovered(self):
        with self.bootstrap_fixture():
            (self.manager / ".git/objects").mkdir(parents=True)
            (self.manager / ".git/objects/partial").write_text("preserve")
            # A checkout can have its entry point but still lack core files.
            (self.manager / "lua/lazy").mkdir(parents=True)
            (self.manager / "lua/lazy/init.lua").write_text(MANAGER)
            output = self.run_bootstrap()
            backups = list(self.manager.parent.glob("*.interrupted"))
            self.assertEqual(len(backups), 1)
            self.assertIn(str(backups[0]), output)
            self.assertEqual((backups[0] / ".git/objects/partial").read_text(), "preserve")
            self.assertTrue((self.manager / "checked-out").is_file())

    def test_failed_commands_and_invalid_checkout_clean_only_private_staging(self):
        for mode, error in (
            ("clone-failure", "git clone failed"),
            ("checkout-failure", "git checkout "),
            ("invalid-checkout", "missing lazy.nvim runtime files"),
        ):
            with self.subTest(mode=mode), self.bootstrap_fixture(mode):
                self.manager.mkdir(parents=True)
                marker = self.manager / "partial"
                marker.write_text("preserve")
                self.run_bootstrap(error)
                self.assertEqual(marker.read_text(), "preserve")
                self.assert_no_staging()

    def test_publication_failure_preserves_backup_and_allows_retry(self):
        with self.bootstrap_fixture("publish-failure"):
            self.manager.mkdir(parents=True)
            (self.manager / "partial").write_text("preserve")
            self.run_bootstrap("fixture publish failure")
            backups = list(self.manager.parent.glob("*.interrupted"))
            self.assertEqual(len(backups), 1)
            self.assertEqual((backups[0] / "partial").read_text(), "preserve")
            self.assertFalse(self.manager.exists())
            self.assertEqual(list(self.manager.parent.glob("lazy.nvim.bootstrap-*")), backups)
            self.env["BOOTSTRAP_MODE"] = ""
            self.run_bootstrap()

    def test_non_directory_is_preserved(self):
        with self.bootstrap_fixture():
            self.manager.parent.mkdir(parents=True)
            self.manager.write_text("preserve")
            self.run_bootstrap("refusing to replace non-directory")
            self.assertEqual(self.manager.read_text(), "preserve")
            self.assert_no_staging()

    def test_active_and_unverifiable_lock_owners_are_preserved(self):
        for mode in ("active-lock", "unverifiable-lock"):
            with self.subTest(mode=mode), self.bootstrap_fixture(mode):
                lock = self.manager.with_name("lazy.nvim.bootstrap-lock")
                lock.mkdir(parents=True)
                marker = lock / f"owner-{os.getpid()}-fixture"
                marker.write_text("preserve")
                self.run_bootstrap("Another bootstrap may still own it")
                self.assertEqual(marker.read_text(), "preserve")
                self.assertEqual(list(self.manager.parent.iterdir()), [lock])

    def test_legacy_empty_lock_is_recovered(self):
        with self.bootstrap_fixture():
            lock = self.manager.with_name("lazy.nvim.bootstrap-lock")
            lock.mkdir(parents=True)
            self.run_bootstrap()
            self.assertTrue((self.manager / "checked-out").is_file())
            self.assert_no_staging()

    def test_stale_reaper_cannot_remove_a_successor_lock(self):
        with self.bootstrap_fixture("successor-lock"):
            lock = self.write_dead_lock()
            self.run_bootstrap("Another bootstrap may still own it")
            owners = list(lock.iterdir())
            self.assertEqual(len(owners), 1)
            self.assertRegex(owners[0].name, r"^owner-\d+-successor$")
            self.assertEqual(owners[0].read_text(), "preserve\n")
            self.assertEqual(list(self.manager.parent.iterdir()), [lock])

    def test_killed_publication_owner_is_recovered(self):
        for mode in ("crash-after-lock", "crash-after-backup"):
            with self.subTest(mode=mode), self.bootstrap_fixture(mode):
                self.manager.mkdir(parents=True)
                (self.manager / "partial").write_text("preserve")
                process = subprocess.run(
                    self.command, cwd=self.root, env=self.env,
                    text=True, capture_output=True, timeout=30,
                )
                self.assertNotEqual(process.returncode, 0)
                lock = self.manager.with_name("lazy.nvim.bootstrap-lock")
                owners = list(lock.iterdir())
                self.assertEqual(len(owners), 1)
                self.assertRegex(owners[0].name, r"^owner-\d+-lazy\.nvim\.bootstrap-")
                self.env["BOOTSTRAP_MODE"] = ""
                self.run_bootstrap()
                self.assertFalse(lock.exists())
                self.assertTrue((self.manager / "checked-out").is_file())
                backups = list(self.manager.parent.glob("*.interrupted"))
                self.assertEqual(len(backups), 1)
                self.assertEqual((backups[0] / "partial").read_text(), "preserve")

    def test_concurrent_bootstraps_publish_once(self):
        for interrupted, dead_owner in ((False, False), (True, False), (True, True)):
            with self.subTest(interrupted=interrupted, dead_owner=dead_owner), self.bootstrap_fixture("concurrent"):
                if interrupted:
                    self.manager.mkdir(parents=True)
                    (self.manager / "partial").write_text("preserve")
                if dead_owner:
                    self.write_dead_lock()
                processes = []
                try:
                    for _ in range(2):
                        processes.append(subprocess.Popen(
                            self.command, cwd=self.root, env=self.env,
                            text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                        ))
                    for process in processes:
                        stdout, stderr = process.communicate(timeout=30)
                        self.assertEqual(process.returncode, 0, stdout + stderr)
                finally:
                    for process in processes:
                        if process.poll() is None:
                            process.kill()
                            process.communicate()
                self.assertEqual(len(self.git_calls()), 4)
                self.assertTrue((self.manager / "checked-out").is_file())
                backups = list(self.manager.parent.glob("*.interrupted"))
                self.assertEqual(len(backups), int(interrupted))
                if interrupted:
                    self.assertEqual((backups[0] / "partial").read_text(), "preserve")
                self.assertEqual(list(self.manager.parent.glob("lazy.nvim.bootstrap-*")), backups)


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
