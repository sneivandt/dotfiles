"""Tests for hook wiring, hook execution modes, release guards, and pre-push protection."""

from __future__ import annotations

from pathlib import Path
import re
import shutil
import tempfile
import unittest

from .helpers import REPO_ROOT, clean_git_env, run


class HookBehaviorTests(unittest.TestCase):
    def test_hook_wiring(self) -> None:
        pre_commit = (REPO_ROOT / "hooks" / "pre-commit").read_text(encoding="utf-8")
        for script in (
            "check-sensitive.sh",
            "check-rust.sh",
            "check-ci-guards.sh",
            "staged-tree.sh",
        ):
            with self.subTest(script=script):
                script_path = REPO_ROOT / "hooks" / script
                self.assertTrue(script_path.is_file(), f"Missing hook helper {script}")
                if script != "staged-tree.sh":
                    self.assertIn(script, pre_commit, f"pre-commit does not call {script}")
                syntax = run(["sh", "-n", str(script_path)], cwd=REPO_ROOT)
                self.assertEqual(
                    syntax.returncode,
                    0,
                    f"{script} has invalid POSIX shell syntax:\n{syntax.stderr}",
                )

    def test_hook_modes(self) -> None:
        with tempfile.TemporaryDirectory() as repo_str:
            repo = Path(repo_str)
            env = clean_git_env(repo)
            self.assertEqual(run(["git", "init", "-q", str(repo)], cwd=repo, env=env).returncode, 0)
            (repo / "hooks").mkdir()
            shutil.copy2(REPO_ROOT / "hooks" / "pre-commit", repo / "hooks" / "pre-commit")
            hook_log = repo / "hook.log"

            for script in ("check-sensitive.sh", "check-rust.sh", "check-ci-guards.sh"):
                (repo / "hooks" / script).write_text(
                    "#!/bin/sh\n"
                    f"printf '%s full=%s\\n' '{script}' \"${{DOTFILES_HOOKS_FULL:-0}}\" >> \"$HOOK_LOG\"\n",
                    encoding="utf-8",
                )

            default_env = dict(env, HOOK_LOG=str(hook_log))
            default_env.pop("DOTFILES_HOOKS_FULL", None)
            res = run(["sh", "hooks/pre-commit"], cwd=repo, env=default_env)
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
            self.assertEqual(
                hook_log.read_text(encoding="utf-8").splitlines(),
                [
                    "check-sensitive.sh full=0",
                    "check-rust.sh full=0",
                    "check-ci-guards.sh full=0",
                ],
            )

            hook_log.write_text("", encoding="utf-8")
            full_env = dict(env, HOOK_LOG=str(hook_log), DOTFILES_HOOKS_FULL="1")
            res = run(["sh", "hooks/pre-commit"], cwd=repo, env=full_env)
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
            self.assertEqual(
                hook_log.read_text(encoding="utf-8").splitlines(),
                [
                    "check-sensitive.sh full=1",
                    "check-rust.sh full=1",
                    "check-ci-guards.sh full=1",
                ],
            )

    def test_release_workflow_guards(self) -> None:
        original_release = (REPO_ROOT / ".github" / "workflows" / "release.yml").read_text(
            encoding="utf-8"
        )
        with tempfile.TemporaryDirectory() as repo_str:
            repo = Path(repo_str)
            env = clean_git_env(repo)
            self.assertEqual(run(["git", "init", "-q", str(repo)], cwd=repo, env=env).returncode, 0)
            (repo / "hooks").mkdir()
            (repo / ".github" / "workflows" / "scripts" / "linux").mkdir(parents=True)
            shutil.copy2(
                REPO_ROOT / "hooks" / "check-ci-guards.sh", repo / "hooks" / "check-ci-guards.sh"
            )
            shutil.copy2(REPO_ROOT / "hooks" / "staged-tree.sh", repo / "hooks" / "staged-tree.sh")
            for helper in (
                "check-ci-contract.py",
                "check-structured-files.py",
                "ci_metadata.py",
                "workflow_yaml.py",
            ):
                shutil.copy2(
                    REPO_ROOT / ".github" / "workflows" / "scripts" / "linux" / helper,
                    repo / ".github" / "workflows" / "scripts" / "linux" / helper,
                )
            release_path = repo / ".github" / "workflows" / "release.yml"

            release_path.write_text(original_release, encoding="utf-8")
            self.assertEqual(
                run(["git", "add", ".github/workflows/release.yml"], cwd=repo, env=env).returncode,
                0,
            )
            res = run(["sh", "hooks/check-ci-guards.sh"], cwd=repo, env=env)
            self.assertEqual(
                res.returncode,
                0,
                f"Hardened release workflow was rejected:\n{res.stdout}{res.stderr}",
            )

            mutations = [
                (
                    "manual workflow_dispatch",
                    re.sub(
                        r"(?m)^([ \t]*workflow_run:)",
                        "  workflow_dispatch:\n\\1",
                        original_release,
                        count=1,
                    ),
                ),
                (
                    "missing attestation verification",
                    re.sub(
                        r"(?ms)^[ \t]*- name: Verify attestation discoverability[ \t]*\n.*?(?=^[ \t]*- name: Create release[ \t]*$)",
                        "",
                        original_release,
                    ),
                ),
                (
                    "missing target_commitish",
                    re.sub(r"(?m)^[ \t]*target_commitish:[ \t]*.*\n", "", original_release),
                ),
                (
                    "missing one-day retention",
                    re.sub(r"(?m)^[ \t]*retention-days:[ \t]*1[ \t]*\n", "", original_release),
                ),
            ]
            for label, mutated in mutations:
                with self.subTest(mutation=label):
                    self.assertNotEqual(
                        mutated, original_release, f"Mutation did not alter release.yml: {label}"
                    )
                    release_path.write_text(mutated, encoding="utf-8")
                    self.assertEqual(
                        run(
                            ["git", "add", ".github/workflows/release.yml"], cwd=repo, env=env
                        ).returncode,
                        0,
                    )
                    res = run(["sh", "hooks/check-ci-guards.sh"], cwd=repo, env=env)
                    self.assertNotEqual(
                        res.returncode, 0, f"Release workflow guards accepted {label}"
                    )

            # Staged unsafe workflow must still fail even if the working tree is restored.
            no_group = re.sub(r"(?m)^[ \t]*group:[ \t]*release[ \t]*\n", "", original_release)
            self.assertNotEqual(no_group, original_release)
            release_path.write_text(no_group, encoding="utf-8")
            self.assertEqual(
                run(["git", "add", ".github/workflows/release.yml"], cwd=repo, env=env).returncode,
                0,
            )
            release_path.write_text(original_release, encoding="utf-8")
            res = run(["sh", "hooks/check-ci-guards.sh"], cwd=repo, env=env)
            self.assertNotEqual(
                res.returncode,
                0,
                "Release workflow guards read the safe working tree instead of the unsafe index",
            )

    def test_pre_push_protection(self) -> None:
        hook = REPO_ROOT / "symlinks" / "config" / "git" / "templates" / "hooks.local" / "pre-push"
        sha = "1" * 40
        zero = "0" * 40

        with tempfile.TemporaryDirectory() as repo_str:
            repo = Path(repo_str)
            env = clean_git_env(repo)
            self.assertEqual(run(["git", "init", "-q", str(repo)], cwd=repo, env=env).returncode, 0)
            run(["git", "config", "github.user", "ci-owner"], cwd=repo, env=env)
            run(
                ["git", "remote", "add", "origin", "https://github.com/ci-owner/repo.git"],
                cwd=repo,
                env=env,
            )
            run(["git", "remote", "add", "upstream", str(repo / "offline.git")], cwd=repo, env=env)
            run(
                ["git", "symbolic-ref", "refs/remotes/upstream/HEAD", "refs/remotes/upstream/main"],
                cwd=repo,
                env=env,
            )

            blocked = run(
                ["sh", str(hook), "upstream", str(repo / "offline.git")],
                cwd=repo,
                env=env,
                stdin_text=f"refs/heads/feature {sha} refs/heads/main {zero}\n",
            )
            self.assertNotEqual(
                blocked.returncode, 0, "Pre-push hook allowed protected branch on upstream remote"
            )

            owned = run(
                ["sh", str(hook), "origin", "https://github.com/ci-owner/repo.git"],
                cwd=repo,
                env=env,
                stdin_text=f"refs/heads/main {sha} refs/heads/main {zero}\n",
            )
            self.assertEqual(
                owned.returncode, 0, f"Pre-push hook blocked owned repo:\n{owned.stderr}"
            )

            feature = run(
                ["sh", str(hook), "upstream", str(repo / "offline.git")],
                cwd=repo,
                env=env,
                stdin_text=f"refs/heads/feature {sha} refs/heads/feature {zero}\n",
            )
            self.assertEqual(
                feature.returncode,
                0,
                f"Pre-push hook blocked non-protected branch:\n{feature.stderr}",
            )
