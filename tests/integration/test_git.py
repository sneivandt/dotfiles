#!/usr/bin/env python3
"""Cross-platform Git configuration, aliases, behavior, clean, hooks, and delta tests.

Usage: python3 -B tests/integration/test_git.py [--strict] [config] [aliases] [behavior] [clean] [hooks] [delta]
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

EXPECTED_CONFIG = (
    ("init.defaultBranch", "main"),
    ("pull.rebase", "true"),
    ("rebase.updateRefs", "true"),
    ("merge.conflictstyle", "zdiff3"),
    ("push.default", "simple"),
    ("push.autoSetupRemote", "true"),
    ("push.useForceIfIncludes", "true"),
    ("diff.algorithm", "histogram"),
)
EXPECTED_ALIASES = ("st", "br", "lo", "ci")


def home_dir() -> Path:
    if sys.platform == "win32":
        value = os.environ.get("USERPROFILE") or os.environ.get("HOME")
    else:
        value = os.environ.get("HOME")
    return Path(value) if value else Path.home()


def run_git(
    *args: str,
    cwd: Path,
    stdin_text: str | None = None,
) -> subprocess.CompletedProcess[str]:
    env = {k: v for k, v in os.environ.items() if k != "GIT_TEMPLATE_DIR"}
    return subprocess.run(
        ["git", *args],
        cwd=cwd,
        env=env,
        input=stdin_text,
        text=True,
        capture_output=True,
        check=False,
    )


def _find_git_shell() -> str | None:
    if sys.platform == "win32":
        git_path = shutil.which("git")
        if git_path:
            git_dir = Path(git_path).resolve().parent
            for candidate in (
                git_dir.parent / "usr" / "bin" / "sh.exe",
                git_dir / "sh.exe",
            ):
                if candidate.is_file():
                    return str(candidate)
    return shutil.which("sh")


def _write_stub(directory: Path, name: str, body: str) -> Path:
    path = directory / name
    path.write_text(f"#!/bin/sh\n{body}\n", encoding="utf-8", newline="\n")
    path.chmod(0o755)
    return path


class GitTests(unittest.TestCase):
    def setUp(self) -> None:
        if shutil.which("git") is None:
            self.skipTest("git not installed")

    def _checked_git(self, *args: str, cwd: Path) -> subprocess.CompletedProcess[str]:
        result = run_git(*args, cwd=cwd)
        self.assertEqual(
            result.returncode,
            0,
            f"git {' '.join(args)} failed: {result.stderr.strip()}",
        )
        return result

    def test_config(self) -> None:
        self._checked_git("--version", cwd=REPO_ROOT)

        home = home_dir()
        config_path = home / ".config" / "git" / "config"
        self.assertTrue(config_path.is_file(), f"custom git config not installed: {config_path}")

        # Evaluate outside any repository so local configuration cannot mask the
        # installed user-level chain (an explicit --global flag skips
        # ~/.config/git/config on Windows).
        with tempfile.TemporaryDirectory() as scratch_str:
            scratch = Path(scratch_str)
            for key, expected in EXPECTED_CONFIG:
                with self.subTest(key=key):
                    result = run_git("config", "--get", key, cwd=scratch)
                    actual = result.stdout.strip() if result.returncode == 0 else ""
                    if actual != expected:
                        origins = run_git("config", "--show-origin", "--get-all", key, cwd=scratch)
                        origin_detail = (
                            origins.stdout.strip()
                            if origins.returncode == 0 and origins.stdout.strip()
                            else f"no file defines {key}"
                        )
                        self.fail(
                            f"{key} expected {expected!r}, got {actual!r} (origins: {origin_detail})"
                        )

            if sys.platform == "win32":
                windows_include = home / ".config" / "git" / "windows"
                self.assertTrue(
                    windows_include.is_file(),
                    f"windows git include not installed: {windows_include}",
                )
                autocrlf = run_git("config", "--get", "core.autocrlf", cwd=scratch)
                actual = autocrlf.stdout.strip() if autocrlf.returncode == 0 else ""
                self.assertEqual(actual, "true", f"core.autocrlf expected 'true', got {actual!r}")

                # Validate the repository-owned Windows fragment, not unrelated
                # runner/system config that may deliberately trust the CI workspace.
                safe_dirs = run_git(
                    "config",
                    "--file",
                    str(windows_include),
                    "--get-all",
                    "safe.directory",
                    cwd=scratch,
                )
                entries = [line.strip() for line in safe_dirs.stdout.splitlines() if line.strip()]
                self.assertNotIn("*", entries, "safe.directory must not trust every repository")

    def test_aliases(self) -> None:
        home = home_dir()
        config_path = home / ".config" / "git" / "config"
        aliases_path = home / ".config" / "git" / "aliases"
        self.assertTrue(config_path.is_file(), f"git config not installed: {config_path}")
        self.assertTrue(aliases_path.is_file(), f"git aliases not installed: {aliases_path}")

        with tempfile.TemporaryDirectory() as scratch_str:
            scratch = Path(scratch_str)
            for alias in EXPECTED_ALIASES:
                with self.subTest(alias=alias):
                    result = run_git("config", "--get", f"alias.{alias}", cwd=scratch)
                    value = result.stdout.strip() if result.returncode == 0 else ""
                    self.assertTrue(value, f"alias.{alias} not defined")

    def test_behavior(self) -> None:
        config_path = home_dir() / ".config" / "git" / "config"
        self.assertTrue(config_path.is_file(), f"git config not installed: {config_path}")

        with tempfile.TemporaryDirectory() as repo_str:
            repo = Path(repo_str)
            self._checked_git("init", str(repo), cwd=repo)
            self._checked_git("config", "user.name", "CI Test", cwd=repo)
            self._checked_git("config", "user.email", "ci@test.local", cwd=repo)

            branch = run_git("branch", "--show-current", cwd=repo).stdout.strip()
            self.assertEqual(branch, "main", f"default branch is {branch!r}, expected 'main'")

            (repo / "test.txt").write_text("test\n", encoding="utf-8")
            self._checked_git("add", "test.txt", cwd=repo)
            self._checked_git("commit", "-m", "Test commit", cwd=repo)

    def test_clean(self) -> None:
        aliases_source = REPO_ROOT / "symlinks" / "config" / "git" / "aliases"
        with tempfile.TemporaryDirectory() as repo_str:
            repo = Path(repo_str)
            self._checked_git("-c", "init.templateDir=", "init", "-q", str(repo), cwd=repo)
            self._checked_git("config", "include.path", str(aliases_source), cwd=repo)
            info_dir = repo / ".git" / "info"
            info_dir.mkdir(parents=True, exist_ok=True)
            (info_dir / "exclude").write_text(".env\n", encoding="utf-8")
            env_file = repo / ".env"
            untracked = repo / "untracked.txt"
            env_file.write_text("fixture\n", encoding="utf-8")
            untracked.write_text("unfinished work\n", encoding="utf-8")

            dry = run_git("cal", cwd=repo, stdin_text="")
            self.assertEqual(dry.returncode, 0, f"git cal with empty stdin failed: {dry.stderr}")
            self.assertTrue(
                env_file.is_file() and untracked.is_file(),
                "git cal deleted files without confirmation",
            )

            confirm = run_git("cal", cwd=repo, stdin_text="1\n")
            self.assertEqual(
                confirm.returncode, 0, f"git cal with confirmation failed: {confirm.stderr}"
            )
            self.assertFalse(
                env_file.exists() or untracked.exists(),
                "git cal did not clean files after explicit confirmation",
            )

    def test_hooks(self) -> None:
        templates = home_dir() / ".config" / "git" / "templates"
        for rel in (
            "hooks/pre-push",
            "hooks/post-merge",
            "hooks.local/pre-push",
            "hooks.local/post-merge",
        ):
            self.assertTrue(
                (templates / rel).is_file(),
                f"missing installed git template hook: {templates / rel}",
            )

        with tempfile.TemporaryDirectory() as base_str:
            base = Path(base_str)
            remote = base / "remote.git"
            repo = base / "work"
            other = base / "other"
            remote.mkdir()
            repo.mkdir()

            self._checked_git(
                "-c",
                "init.templateDir=",
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                str(remote),
                cwd=remote,
            )
            self._checked_git("init", "-q", "-b", "main", str(repo), cwd=repo)
            self.assertTrue(
                (repo / ".git" / "hooks" / "pre-push").is_file(),
                "git init did not copy pre-push hook from init.templateDir",
            )
            self._checked_git("config", "user.name", "CI Test", cwd=repo)
            self._checked_git("config", "user.email", "ci@test.local", cwd=repo)
            self._checked_git("remote", "add", "origin", str(remote), cwd=repo)

            (repo / "README.md").write_text("initial\n", encoding="utf-8")
            self._checked_git("add", "README.md", cwd=repo)
            self._checked_git("commit", "-q", "-m", "Initial commit", cwd=repo)

            blocked = run_git("push", "origin", "main", cwd=repo)
            self.assertNotEqual(
                blocked.returncode, 0, "pre-push hook allowed pushing directly to main"
            )
            self.assertIn("protected branch main", blocked.stderr)

            self._checked_git("push", "--no-verify", "-q", "origin", "main", cwd=repo)

            self._checked_git("checkout", "-q", "-b", "feature", cwd=repo)
            (repo / "feature.txt").write_text("feature\n", encoding="utf-8")
            self._checked_git("add", "feature.txt", cwd=repo)
            self._checked_git("commit", "-q", "-m", "Feature commit", cwd=repo)
            self._checked_git("push", "-q", "origin", "feature", cwd=repo)

            self._checked_git("checkout", "-q", "main", cwd=repo)
            self._checked_git("branch", "merged-topic", cwd=repo)
            self._checked_git("checkout", "-q", "-b", "unmerged-topic", cwd=repo)
            (repo / "wip.txt").write_text("wip\n", encoding="utf-8")
            self._checked_git("add", "wip.txt", cwd=repo)
            self._checked_git("commit", "-q", "-m", "Unmerged commit", cwd=repo)
            self._checked_git("checkout", "-q", "main", cwd=repo)

            self._checked_git(
                "-c", "init.templateDir=", "clone", "-q", str(remote), str(other), cwd=base
            )
            self._checked_git("config", "user.name", "CI Test", cwd=other)
            self._checked_git("config", "user.email", "ci@test.local", cwd=other)
            (other / "upstream.txt").write_text("upstream\n", encoding="utf-8")
            self._checked_git("add", "upstream.txt", cwd=other)
            self._checked_git("commit", "-q", "-m", "Upstream commit", cwd=other)
            self._checked_git("push", "--no-verify", "-q", "origin", "main", cwd=other)

            self._checked_git("fetch", "-q", "origin", cwd=repo)
            self._checked_git("remote", "set-head", "origin", "main", cwd=repo)
            self._checked_git("merge", "-q", "--ff-only", "origin/main", cwd=repo)

            self.assertNotEqual(
                run_git(
                    "show-ref", "--verify", "--quiet", "refs/heads/merged-topic", cwd=repo
                ).returncode,
                0,
                "post-merge hook did not clean up merged-topic branch",
            )
            self.assertEqual(
                run_git(
                    "show-ref", "--verify", "--quiet", "refs/heads/unmerged-topic", cwd=repo
                ).returncode,
                0,
                "post-merge hook unexpectedly deleted unmerged-topic branch",
            )

            # Also exercise the delete-merged alias via the shared helper against HEAD.
            aliases_source = REPO_ROOT / "symlinks" / "config" / "git" / "aliases"
            self._checked_git("config", "include.path", str(aliases_source), cwd=repo)
            self._checked_git("branch", "local-merged", cwd=repo)
            self._checked_git("delete-merged", cwd=repo)
            self.assertNotEqual(
                run_git(
                    "show-ref", "--verify", "--quiet", "refs/heads/local-merged", cwd=repo
                ).returncode,
                0,
                "git delete-merged did not clean up local-merged branch",
            )
            self.assertEqual(
                run_git(
                    "show-ref", "--verify", "--quiet", "refs/heads/unmerged-topic", cwd=repo
                ).returncode,
                0,
                "git delete-merged unexpectedly deleted unmerged-topic branch",
            )

    def test_delta(self) -> None:
        shell = _find_git_shell()
        if shell is None:
            self.skipTest("POSIX sh not available")

        delta_config = REPO_ROOT / "symlinks" / "config" / "git" / "delta"
        self.assertTrue(delta_config.is_file(), f"delta config missing: {delta_config}")

        pager = self._checked_git(
            "config", "--file", str(delta_config), "--get", "core.pager", cwd=REPO_ROOT
        ).stdout.strip()
        diff_filter = self._checked_git(
            "config", "--file", str(delta_config), "--get", "interactive.diffFilter", cwd=REPO_ROOT
        ).stdout.strip()

        def run_cmd(
            command: str,
            bin_dir: Path,
            columns: str | None = None,
        ) -> subprocess.CompletedProcess[str]:
            env = {
                "PATH": str(bin_dir),
                "LC_ALL": "C",
            }
            if sys.platform == "win32":
                for key in ("SYSTEMROOT", "WINDIR", "TEMP", "TMP"):
                    if key in os.environ:
                        env[key] = os.environ[key]
            if columns is not None:
                env["COLUMNS"] = columns
            kwargs: dict[str, object] = {}
            if sys.platform != "win32":
                # Detach from any controlling tty so </dev/tty falls through to COLUMNS.
                kwargs["start_new_session"] = True
            return subprocess.run(
                [shell, "-c", command],
                env=env,
                stdin=subprocess.DEVNULL,
                text=True,
                capture_output=True,
                check=False,
                timeout=15,
                **kwargs,
            )

        with tempfile.TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            without_delta = tmp / "without-delta"
            with_delta = tmp / "with-delta"
            without_delta.mkdir()
            with_delta.mkdir()

            for directory in (without_delta, with_delta):
                _write_stub(directory, "less", 'printf "LESS:%s\\n" "$*"')
                _write_stub(directory, "cat", 'printf "CAT:%s\\n" "$*"')
            _write_stub(with_delta, "delta", 'printf "DELTA:%s\\n" "$*"')

            fallback_pager = run_cmd(pager, without_delta)
            self.assertEqual(fallback_pager.returncode, 0, fallback_pager.stderr)
            self.assertEqual(fallback_pager.stdout.strip(), "LESS:-imRj8X")

            fallback_filter = run_cmd(diff_filter, without_delta)
            self.assertEqual(fallback_filter.returncode, 0, fallback_filter.stderr)
            self.assertEqual(fallback_filter.stdout.strip(), "CAT:")

            narrow = run_cmd(pager, with_delta, columns="159")
            self.assertEqual(narrow.returncode, 0, narrow.stderr)
            self.assertEqual(narrow.stdout.strip(), "DELTA:")

            wide = run_cmd(pager, with_delta, columns="160")
            self.assertEqual(wide.returncode, 0, wide.stderr)
            self.assertEqual(wide.stdout.strip(), "DELTA:--side-by-side")

            invalid_cols = run_cmd(pager, with_delta, columns="invalid")
            self.assertEqual(invalid_cols.returncode, 0, invalid_cols.stderr)
            self.assertEqual(invalid_cols.stdout.strip(), "DELTA:")

            unset_cols = run_cmd(pager, with_delta)
            self.assertEqual(unset_cols.returncode, 0, unset_cols.stderr)
            self.assertEqual(unset_cols.stdout.strip(), "DELTA:")

            delta_filter = run_cmd(diff_filter, with_delta)
            self.assertEqual(delta_filter.returncode, 0, delta_filter.stderr)
            self.assertEqual(delta_filter.stdout.strip(), "DELTA:--color-only")


SELECTORS = {
    "config": "test_config",
    "alias": "test_aliases",
    "aliases": "test_aliases",
    "behavior": "test_behavior",
    "clean": "test_clean",
    "hooks": "test_hooks",
    "delta": "test_delta",
}


def main(argv: list[str]) -> int:
    strict, remaining = pop_strict_flag(argv)
    selected = (
        [name.lower() for name in remaining]
        if remaining
        else ["config", "aliases", "behavior", "clean", "hooks", "delta"]
    )
    suite = unittest.TestSuite()
    for name in selected:
        method = SELECTORS.get(name)
        if method is None:
            print(f"ERROR: unknown git test: {name}", file=sys.stderr)
            return 2
        suite.addTest(GitTests(method))
    return run_suite(suite, strict=strict)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
