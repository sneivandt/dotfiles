#!/usr/bin/env python3
"""Exercise Git templates and cleanup commands without changing the host."""

import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[4]
TEMPLATES = ROOT / "symlinks/config/git/templates"


class GitTemplatesTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="dotfiles-git-maintenance-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.home = self.root / "home"
        self.templates = self.home / ".config/git/templates"
        shutil.copytree(TEMPLATES, self.templates)
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
        self.env.update(HOME=str(self.home), GIT_CONFIG_GLOBAL="/dev/null",
                        GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_COUNT="0",
                        GIT_AUTHOR_NAME="Test", GIT_AUTHOR_EMAIL="ci@test.local",
                        GIT_COMMITTER_NAME="Test", GIT_COMMITTER_EMAIL="ci@test.local")
        self.repo = self.root / "repo"
        self.git("init", "-q", "-b", "main", "--template=" + str(self.templates), str(self.repo), cwd=self.root)
        self.git("commit", "--allow-empty", "-qm", "base")

    def git(self, *args, cwd=None):
        result = subprocess.run(["git", *args], cwd=cwd or self.repo,
                                env=self.env, text=True, capture_output=True, check=True)
        return result.stdout.strip()

    def push(self, branch, remote="upstream", url=None, prefix=""):
        payload = prefix + f"refs/heads/local {'1' * 40} refs/heads/{branch} {'0' * 40}\n"
        return subprocess.run([str(self.repo / ".git/hooks/pre-push"), remote,
                               url or str(self.root / "missing-remote")],
                              cwd=self.repo, env=self.env, input=payload,
                              text=True, capture_output=True).returncode

    def test_destination_default_and_configured_branches(self):
        self.git("symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main")
        self.git("symbolic-ref", "refs/remotes/upstream/HEAD", "refs/remotes/upstream/trunk")
        self.git("config", "--add", "dotfiles.protectedBranch", "release/stable")
        self.git("config", "--add", "dotfiles.protectedBranch", "dev")
        for branch in ["main", "master", "develop", "trunk", "release/stable", "dev"]:
            with self.subTest(branch=branch):
                self.assertEqual(self.push(branch), 1)
        self.assertEqual(self.push("feature"), 0)
        self.assertEqual(self.push("release/stable-fix"), 0)
        prefix = f"refs/heads/feature {'1' * 40} refs/heads/feature {'0' * 40}\n"
        self.assertEqual(self.push("trunk", prefix=prefix), 1)

    def test_uncached_default_discovery_preserves_push_input(self):
        remote = self.root / "remote.git"
        self.git("init", "--bare", "-q", "-b", "trunk", str(remote))
        self.git("push", "-q", str(remote), "main:trunk")
        self.assertEqual(self.push("trunk", url=str(remote)), 1)
        self.assertEqual(self.push("feature", url=str(remote)), 0)

    def test_owned_github_repository_is_exempt(self):
        self.git("config", "github.user", "test-owner")
        ssh_host = "git@" + "github.com"
        for url in ["https://github.com/test-owner/repo.git",
                    f"{ssh_host}:test-owner/repo.git",
                    f"ssh://{ssh_host}/test-owner/repo.git"]:
            with self.subTest(url=url):
                self.assertEqual(self.push("main", remote="origin", url=url), 0)

    def test_merge_and_rebase_cleanup_preserve_work(self):
        remote = self.root / "remote"
        self.git("clone", "-q", "--template=", str(self.repo), str(remote))
        # The remote advances independently while the clone has local work.
        clone = self.root / "clone"
        self.git("clone", "-q", "--template=" + str(self.templates), str(remote), str(clone))
        self.git("branch", "old", cwd=clone)
        self.git("branch", "dev", cwd=clone)
        self.git("worktree", "add", "-q", "-b", "held", str(self.root / "held"), cwd=clone)
        self.git("switch", "-qc", "unmerged", cwd=clone)
        (clone / "unmerged").write_text("unmerged work")
        self.git("add", "unmerged", cwd=clone)
        self.git("commit", "-qm", "unmerged", cwd=clone)
        self.git("switch", "-q", "main", cwd=clone)
        # Amend triggers post-rewrite but must not trigger branch cleanup.
        (clone / "local").write_text("local work")
        self.git("add", "local", cwd=clone)
        self.git("commit", "-qm", "local", cwd=clone)
        self.git("commit", "--amend", "--no-edit", cwd=clone)
        self.assertIn("old", self.git("branch", "--format=%(refname:short)", cwd=clone).splitlines())
        (remote / "remote").write_text("remote work")
        self.git("add", "remote", cwd=remote)
        self.git("commit", "-qm", "remote", cwd=remote)
        self.git("fetch", "-q", cwd=clone)
        self.git("branch", "refused", "origin/main", cwd=clone)
        self.git("branch", "--set-upstream-to=unmerged", "refused", cwd=clone)
        self.git("-c", "pull.rebase=true", "pull", "-q", cwd=clone)
        branches = self.git("branch", "--format=%(refname:short)", cwd=clone).splitlines()
        self.assertNotIn("old", branches)
        for branch in ["main", "dev", "held", "unmerged", "refused"]:
            self.assertIn(branch, branches)
        self.git("branch", "old-merge", "origin/main", cwd=clone)
        (remote / "next").write_text("next")
        self.git("add", "next", cwd=remote)
        self.git("commit", "-qm", "next", cwd=remote)
        self.git("-c", "pull.rebase=false", "pull", "-q", "--no-edit", cwd=clone)
        self.assertNotIn("old-merge", self.git("branch", "--format=%(refname:short)", cwd=clone).splitlines())

    def test_cleanup_without_origin_is_a_noop(self):
        self.git("branch", "keep")
        subprocess.run([str(self.repo / ".git/hooks/post-merge"), "0"],
                       cwd=self.repo, env=self.env, check=True)
        self.assertIn("keep", self.git("branch", "--format=%(refname:short)").splitlines())


class CleanupTests(unittest.TestCase):
    def test_retention_for_files_links_and_directories(self):
        for name, directory, days in [("home-tmp", "tmp", 7), ("screenshots", "screenshots", 30)]:
            with self.subTest(service=name), tempfile.TemporaryDirectory(prefix="dotfiles-cleanup-") as tmp:
                home = Path(tmp)
                target = home / directory
                target.mkdir()
                oldtime = time.time() - (days + 2) * 86400
                for age in ["old", "new"]:
                    (target / f"{age}-file").write_text("fixture")
                    (target / f"{age}-directory").mkdir()
                    (target / f"{age}-link").symlink_to(home / "outside")
                    if age == "old":
                        for path in target.iterdir():
                            if path.name.startswith("old-"):
                                os.utime(path, (oldtime, oldtime), follow_symlinks=False)
                outside = home / "outside"
                outside.mkdir()
                (outside / "keep").write_text("outside the cleanup root")
                unit = ROOT / f"symlinks/config/systemd/user/clean-{name}.service"
                for line in unit.read_text().splitlines():
                    if line.startswith("ExecStart="):
                        subprocess.run(shlex.split(line.removeprefix("ExecStart=").replace("%h", str(home))), check=True)
                self.assertTrue(target.is_dir())
                self.assertTrue((outside / "keep").is_file())
                for kind in ["file", "directory", "link"]:
                    self.assertFalse(os.path.lexists(target / f"old-{kind}"))
                    self.assertTrue(os.path.lexists(target / f"new-{kind}"))


if __name__ == "__main__":
    unittest.main()
