#!/usr/bin/env python3
"""Cross-platform Git configuration, aliases, behavior, cleanup, and hook tests.

Usage: python3 -B tests/git/test-git.py [config] [aliases] [behavior] [clean] [hooks]
"""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

REPO_ROOT = Path(os.environ.get("DIR") or Path(__file__).resolve().parents[2])
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


def require_git() -> bool:
    if shutil.which("git") is None:
        print("Skipping: git not installed")
        return False
    return True


def fail(message: str) -> None:
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def test_config() -> None:
    if not require_git():
        return
    print("=== Testing git configuration")

    version = run_git("--version", cwd=REPO_ROOT)
    if version.returncode != 0:
        fail("git --version failed")

    home = home_dir()
    config_path = home / ".config" / "git" / "config"
    if not config_path.is_file():
        fail(f"custom git config not installed: {config_path}")
    print(f"PASS: custom git config found: {config_path}")

    # Evaluate outside any repository so local configuration cannot mask the
    # installed user-level chain (an explicit --global flag skips
    # ~/.config/git/config on Windows).
    with tempfile.TemporaryDirectory() as scratch_str:
        scratch = Path(scratch_str)
        errors = 0
        for key, expected in EXPECTED_CONFIG:
            result = run_git("config", "--get", key, cwd=scratch)
            actual = result.stdout.strip() if result.returncode == 0 else ""
            if actual == expected:
                print(f"PASS: {key} = {actual}")
            else:
                print(
                    f"ERROR: {key} expected '{expected}', got '{actual}'",
                    file=sys.stderr,
                )
                origins = run_git("config", "--show-origin", "--get-all", key, cwd=scratch)
                if origins.returncode == 0 and origins.stdout.strip():
                    for line in origins.stdout.splitlines():
                        print(f"    origin: {line}", file=sys.stderr)
                else:
                    print(f"    origin: no file defines {key}", file=sys.stderr)
                errors += 1

        if sys.platform == "win32":
            windows_include = home / ".config" / "git" / "windows"
            if not windows_include.is_file():
                fail(f"windows git include not installed: {windows_include}")
            print(f"PASS: windows git include found: {windows_include}")

            autocrlf = run_git("config", "--get", "core.autocrlf", cwd=scratch)
            actual = autocrlf.stdout.strip() if autocrlf.returncode == 0 else ""
            if actual != "true":
                print(
                    f"ERROR: core.autocrlf expected 'true', got '{actual}'",
                    file=sys.stderr,
                )
                errors += 1
            else:
                print(f"PASS: core.autocrlf = {actual}")

            # Validate the repository-owned Windows fragment, not unrelated
            # runner/system config that may deliberately trust the CI workspace.
            safe_dirs = run_git(
                "config", "--file", str(windows_include), "--get-all", "safe.directory",
                cwd=scratch,
            )
            entries = [line.strip() for line in safe_dirs.stdout.splitlines() if line.strip()]
            if "*" in entries:
                print("ERROR: safe.directory must not trust every repository", file=sys.stderr)
                errors += 1
            else:
                print("PASS: managed safe.directory contains no wildcard")

        if errors:
            raise SystemExit(1)


def test_aliases() -> None:
    if not require_git():
        return
    print("=== Testing git aliases")

    home = home_dir()
    config_path = home / ".config" / "git" / "config"
    aliases_path = home / ".config" / "git" / "aliases"
    if not config_path.is_file():
        fail(f"git config not installed: {config_path}")
    if not aliases_path.is_file():
        fail(f"git aliases not installed: {aliases_path}")

    with tempfile.TemporaryDirectory() as scratch_str:
        scratch = Path(scratch_str)
        errors = 0
        for alias in EXPECTED_ALIASES:
            result = run_git("config", "--get", f"alias.{alias}", cwd=scratch)
            value = result.stdout.strip() if result.returncode == 0 else ""
            if value:
                print(f"PASS: alias.{alias} = {value}")
            else:
                print(f"ERROR: alias.{alias} not defined", file=sys.stderr)
                errors += 1
        if errors:
            raise SystemExit(1)


def test_behavior() -> None:
    if not require_git():
        return
    print("=== Testing git behavior")

    config_path = home_dir() / ".config" / "git" / "config"
    if not config_path.is_file():
        fail(f"git config not installed: {config_path}")

    with tempfile.TemporaryDirectory() as repo_str:
        repo = Path(repo_str)
        init = run_git("init", str(repo), cwd=repo)
        if init.returncode != 0:
            fail(f"git init failed: {init.stderr.strip()}")
        run_git("config", "user.name", "CI Test", cwd=repo)
        run_git("config", "user.email", "ci@test.local", cwd=repo)

        branch = run_git("branch", "--show-current", cwd=repo).stdout.strip()
        if branch != "main":
            fail(f"default branch is '{branch}', expected 'main'")
        print("PASS: default branch is main")

        (repo / "test.txt").write_text("test\n", encoding="utf-8")
        add = run_git("add", "test.txt", cwd=repo)
        if add.returncode != 0:
            fail(f"git add failed: {add.stderr.strip()}")
        commit = run_git("commit", "-m", "Test commit", cwd=repo)
        if commit.returncode != 0:
            fail(f"commit failed: {commit.stderr.strip()}")
        print("PASS: commit created successfully")


def test_clean() -> None:
    if not require_git():
        return
    print("=== Testing interactive git cleanup")

    aliases_source = REPO_ROOT / "symlinks" / "config" / "git" / "aliases"
    with tempfile.TemporaryDirectory() as repo_str:
        repo = Path(repo_str)
        init = run_git("-c", "init.templateDir=", "init", "-q", str(repo), cwd=repo)
        if init.returncode != 0:
            fail(f"git init failed: {init.stderr.strip()}")
        run_git("config", "include.path", str(aliases_source), cwd=repo)
        info_dir = repo / ".git" / "info"
        info_dir.mkdir(parents=True, exist_ok=True)
        (info_dir / "exclude").write_text(".env\n", encoding="utf-8")
        env_file = repo / ".env"
        untracked = repo / "untracked.txt"
        env_file.write_text("fixture\n", encoding="utf-8")
        untracked.write_text("unfinished work\n", encoding="utf-8")

        dry = run_git("cal", cwd=repo, stdin_text="")
        if dry.returncode != 0:
            fail(f"git cal with empty stdin failed: {dry.stderr.strip()}")
        if not (env_file.is_file() and untracked.is_file()):
            fail("git cal deleted files without confirmation")

        confirm = run_git("cal", cwd=repo, stdin_text="1\n")
        if confirm.returncode != 0:
            fail(f"git cal with confirmation failed: {confirm.stderr.strip()}")
        if env_file.exists() or untracked.exists():
            fail("git cal did not clean files after explicit confirmation")
        print("PASS: cleanup preserves files until explicitly confirmed")


def test_hooks() -> None:
    if not require_git():
        return
    print("=== Testing git template hooks")

    templates = home_dir() / ".config" / "git" / "templates"
    for rel in ("hooks/pre-push", "hooks/post-merge", "hooks.local/pre-push", "hooks.local/post-merge"):
        if not (templates / rel).is_file():
            fail(f"missing installed git template hook: {templates / rel}")

    def checked_git(*args: str, cwd: Path) -> subprocess.CompletedProcess[str]:
        result = run_git(*args, cwd=cwd)
        if result.returncode != 0:
            fail(f"git {' '.join(args)} failed: {result.stderr.strip()}")
        return result

    with tempfile.TemporaryDirectory() as base_str:
        base = Path(base_str)
        remote = base / "remote.git"
        repo = base / "work"
        other = base / "other"
        remote.mkdir()
        repo.mkdir()

        checked_git("-c", "init.templateDir=", "init", "-q", "--bare", "-b", "main", str(remote), cwd=remote)
        checked_git("init", "-q", "-b", "main", str(repo), cwd=repo)
        if not (repo / ".git" / "hooks" / "pre-push").is_file():
            fail("git init did not copy pre-push hook from init.templateDir")
        checked_git("config", "user.name", "CI Test", cwd=repo)
        checked_git("config", "user.email", "ci@test.local", cwd=repo)
        checked_git("remote", "add", "origin", str(remote), cwd=repo)

        (repo / "README.md").write_text("initial\n", encoding="utf-8")
        checked_git("add", "README.md", cwd=repo)
        checked_git("commit", "-q", "-m", "Initial commit", cwd=repo)

        blocked = run_git("push", "origin", "main", cwd=repo)
        if blocked.returncode == 0:
            fail("pre-push hook allowed pushing directly to main")
        if "protected branch main" not in blocked.stderr:
            fail(f"pre-push hook rejection missing expected message: {blocked.stderr.strip()}")

        checked_git("push", "--no-verify", "-q", "origin", "main", cwd=repo)

        checked_git("checkout", "-q", "-b", "feature", cwd=repo)
        (repo / "feature.txt").write_text("feature\n", encoding="utf-8")
        checked_git("add", "feature.txt", cwd=repo)
        checked_git("commit", "-q", "-m", "Feature commit", cwd=repo)
        checked_git("push", "-q", "origin", "feature", cwd=repo)

        checked_git("checkout", "-q", "main", cwd=repo)
        checked_git("branch", "merged-topic", cwd=repo)
        checked_git("checkout", "-q", "-b", "unmerged-topic", cwd=repo)
        (repo / "wip.txt").write_text("wip\n", encoding="utf-8")
        checked_git("add", "wip.txt", cwd=repo)
        checked_git("commit", "-q", "-m", "Unmerged commit", cwd=repo)
        checked_git("checkout", "-q", "main", cwd=repo)

        checked_git("-c", "init.templateDir=", "clone", "-q", str(remote), str(other), cwd=base)
        checked_git("config", "user.name", "CI Test", cwd=other)
        checked_git("config", "user.email", "ci@test.local", cwd=other)
        (other / "upstream.txt").write_text("upstream\n", encoding="utf-8")
        checked_git("add", "upstream.txt", cwd=other)
        checked_git("commit", "-q", "-m", "Upstream commit", cwd=other)
        checked_git("push", "--no-verify", "-q", "origin", "main", cwd=other)

        checked_git("fetch", "-q", "origin", cwd=repo)
        checked_git("remote", "set-head", "origin", "main", cwd=repo)
        checked_git("merge", "-q", "--ff-only", "origin/main", cwd=repo)

        if run_git("show-ref", "--verify", "--quiet", "refs/heads/merged-topic", cwd=repo).returncode == 0:
            fail("post-merge hook did not clean up merged-topic branch")
        if run_git("show-ref", "--verify", "--quiet", "refs/heads/unmerged-topic", cwd=repo).returncode != 0:
            fail("post-merge hook unexpectedly deleted unmerged-topic branch")
        print("PASS: template hooks block protected pushes and clean merged branches")


TESTS = {
    "config": test_config,
    "alias": test_aliases,
    "aliases": test_aliases,
    "behavior": test_behavior,
    "clean": test_clean,
    "hooks": test_hooks,
}


def main(argv: list[str]) -> int:
    if argv:
        selected = [name.lower() for name in argv]
    else:
        selected = ["config", "aliases", "behavior", "clean", "hooks"]

    for name in selected:
        fn = TESTS.get(name)
        if fn is None:
            fail(f"unknown git test: {name}")
        fn()
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
