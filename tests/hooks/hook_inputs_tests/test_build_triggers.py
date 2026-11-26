"""Build-script rerun-trigger tests for cli/build.rs."""

from __future__ import annotations

from pathlib import Path
import re
import shutil
import tempfile
import unittest

from .helpers import REPO_ROOT, init_fixture_repository, run


class BuildTriggerTests(unittest.TestCase):
    def test_build_version_ref_triggers(self) -> None:
        toolchain_toml = (REPO_ROOT / "cli" / "rust-toolchain.toml").read_text(encoding="utf-8")
        match = re.search(r'^channel = "([^"]+)"$', toolchain_toml, re.MULTILINE)
        self.assertIsNotNone(match, "Missing channel in cli/rust-toolchain.toml")
        toolchain = match.group(1)

        if shutil.which("rustup") is None:
            self.skipTest("rustup is not installed")
        which = run(["rustup", "which", "--toolchain", toolchain, "rustc"], cwd=REPO_ROOT)
        if which.returncode != 0 or not which.stdout.strip():
            self.skipTest(f"pinned Rust compiler {toolchain} is not installed")
        compiler = which.stdout.strip()

        with tempfile.TemporaryDirectory() as fixture_str:
            fixture = Path(fixture_str)
            build_exe = fixture / "build-script.exe"
            # Compile only the std-only build script, without Cargo or dependency builds.
            compiled = run(
                [compiler, "--edition=2024", str(REPO_ROOT / "cli" / "build.rs"), "-o", str(build_exe)],
                cwd=fixture,
            )
            self.assertEqual(compiled.returncode, 0, f"rustc failed:\n{compiled.stderr}")

            repo = fixture / "repo"
            env = init_fixture_repository(repo)
            self.assertEqual(run(["git", "symbolic-ref", "HEAD", "refs/heads/main"], cwd=repo, env=env).returncode, 0)
            self.assertEqual(
                run(
                    [
                        "git",
                        "-c", "user.name=Fixture",
                        "-c", "user.email=fixture@test.local",
                        "-c", "core.hooksPath=/dev/null",
                        "commit", "--allow-empty", "-qm", "baseline",
                    ],
                    cwd=repo,
                    env=env,
                ).returncode,
                0,
            )
            main_git_dir = run(["git", "rev-parse", "--absolute-git-dir"], cwd=repo, env=env).stdout.strip()

            run_env = dict(env)
            run_env.pop("DOTFILES_VERSION", None)
            run_env.pop("DOTFILES_RELEASE_REPO", None)

            no_pub = run([str(build_exe)], cwd=repo, env=run_env)
            self.assertEqual(no_pub.returncode, 0, no_pub.stderr)
            self.assertIn("cargo::rustc-env=DOTFILES_RELEASE_REPO=", no_pub.stdout.splitlines())

            pub_env = dict(run_env, DOTFILES_RELEASE_REPO="fork-owner/releases")
            pub = run([str(build_exe)], cwd=repo, env=pub_env)
            self.assertEqual(pub.returncode, 0, pub.stderr)
            pub_lines = pub.stdout.splitlines()
            self.assertIn("cargo::rustc-env=DOTFILES_RELEASE_REPO=fork-owner/releases", pub_lines)
            self.assertIn("cargo::rerun-if-env-changed=DOTFILES_RELEASE_REPO", pub_lines)

            for invalid in ("owner/repo/extra", "https://github.com/owner/repo", "owner/repo?query", "../repo"):
                with self.subTest(invalid=invalid):
                    bad = run([str(build_exe)], cwd=repo, env=dict(run_env, DOTFILES_RELEASE_REPO=invalid))
                    self.assertNotEqual(bad.returncode, 0, f"Build script accepted invalid publisher: {invalid}")

            attached = run([str(build_exe)], cwd=repo, env=run_env)
            self.assertEqual(attached.returncode, 0, attached.stderr)
            attached_lines = attached.stdout.splitlines()
            self.assertIn(f"cargo::rerun-if-changed={main_git_dir}/refs/heads/main", attached_lines)
            self.assertIn(f"cargo::rerun-if-changed={main_git_dir}/HEAD", attached_lines)
            self.assertNotIn(f"cargo::rerun-if-changed={main_git_dir}/refs/", attached_lines)

            worktree = fixture / "worktree"
            self.assertEqual(
                run(["git", "worktree", "add", "-qb", "fixture-branch", str(worktree)], cwd=repo, env=env).returncode,
                0,
            )
            wt_env = dict(run_env)
            for key in ("GIT_DIR", "GIT_COMMON_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"):
                wt_env.pop(key, None)
            linked = run([str(build_exe)], cwd=worktree, env=wt_env)
            self.assertEqual(linked.returncode, 0, linked.stderr)
            worktree_git_dir = run(["git", "rev-parse", "--absolute-git-dir"], cwd=worktree, env=wt_env).stdout.strip()
            linked_lines = linked.stdout.splitlines()
            self.assertIn(f"cargo::rerun-if-changed={worktree_git_dir}/HEAD", linked_lines)
            self.assertIn(f"cargo::rerun-if-changed={main_git_dir}/refs/heads/fixture-branch", linked_lines)

            self.assertEqual(run(["git", "checkout", "--detach", "-q"], cwd=repo, env=env).returncode, 0)
            detached = run([str(build_exe)], cwd=repo, env=run_env)
            self.assertEqual(detached.returncode, 0, detached.stderr)
            detached_lines = detached.stdout.splitlines()
            self.assertIn(f"cargo::rerun-if-changed={main_git_dir}/HEAD", detached_lines)
            self.assertFalse(
                any("refs/heads/" in line for line in detached_lines),
                f"Detached HEAD retained an unrelated branch dependency:\n{detached.stdout}",
            )
