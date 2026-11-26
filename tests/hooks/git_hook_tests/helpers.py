"""Shared disposable-repository helpers for Git hook integration tests."""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess

REPO_ROOT = Path(os.environ.get("DIR") or Path(__file__).resolve().parents[3])

_UNSET_GIT_VARS = (
    "GIT_DIR",
    "GIT_COMMON_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
)


def clean_git_env(repo: Path) -> dict[str, str]:
    env = dict(os.environ)
    for var in _UNSET_GIT_VARS:
        env.pop(var, None)
    empty_cfg = repo / ".empty-gitconfig"
    empty_cfg.write_text("", encoding="utf-8")
    env["GIT_CONFIG_NOSYSTEM"] = "1"
    env["GIT_CONFIG_COUNT"] = "0"
    env["GIT_CONFIG_GLOBAL"] = str(empty_cfg)
    return env


def run(
    command: list[str],
    *,
    cwd: Path,
    env: dict[str, str] | None = None,
    stdin_text: str | None = None,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=cwd,
        env=env,
        input=stdin_text,
        text=True,
        capture_output=True,
        check=False,
    )


def init_hook_repo(repo: Path) -> dict[str, str]:
    repo.mkdir(parents=True, exist_ok=True)
    env = clean_git_env(repo)
    init = run(["git", "init", "-q", "--template=", str(repo)], cwd=repo, env=env)
    if init.returncode != 0:
        raise AssertionError(f"git init failed: {init.stderr}")
    run(["git", "config", "user.name", "Hook Test"], cwd=repo, env=env)
    run(["git", "config", "user.email", "ci@test.local"], cwd=repo, env=env)

    hooks_dst = repo / "hooks"
    shutil.copytree(REPO_ROOT / "hooks", hooks_dst, dirs_exist_ok=True)
    git_hooks = repo / ".git" / "hooks"
    git_hooks.mkdir(parents=True, exist_ok=True)
    pre_commit_src = hooks_dst / "pre-commit"
    pre_commit_src.chmod(0o755)
    pre_commit_link = git_hooks / "pre-commit"
    if pre_commit_link.exists() or pre_commit_link.is_symlink():
        pre_commit_link.unlink()
    os.symlink(pre_commit_src, pre_commit_link)

    # Establish a parent commit so clean-commit tests can safely reset HEAD~1.
    (repo / "baseline.txt").write_text("baseline\n", encoding="utf-8")
    run(["git", "add", "baseline.txt"], cwd=repo, env=env)
    baseline = run(
        ["git", "-c", "core.hooksPath=/dev/null", "commit", "-qm", "Baseline"],
        cwd=repo,
        env=env,
    )
    if baseline.returncode != 0:
        raise AssertionError(f"baseline commit failed: {baseline.stderr}")
    return env
