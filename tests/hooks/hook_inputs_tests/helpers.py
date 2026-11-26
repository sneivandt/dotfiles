"""Shared fixture helpers for staged hook-input regression tests."""

from __future__ import annotations

import os
from pathlib import Path
import subprocess

REPO_ROOT = Path(os.environ.get("DIR") or Path(__file__).resolve().parents[3])

_STRIP_GIT_VARS = (
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
)


def isolated_git_env(repo: Path, base: dict[str, str] | None = None) -> dict[str, str]:
    """Pin every Git subprocess to ``repo`` so caller Git state cannot leak."""
    env = dict(os.environ if base is None else base)
    for var in _STRIP_GIT_VARS:
        env.pop(var, None)
    git_dir = repo / ".git"
    empty_cfg = repo / ".empty-gitconfig"
    empty_cfg.write_text("", encoding="utf-8")
    env["GIT_DIR"] = str(git_dir)
    env["GIT_COMMON_DIR"] = str(git_dir)
    env["GIT_WORK_TREE"] = str(repo)
    env["GIT_INDEX_FILE"] = str(git_dir / "index")
    env["GIT_CONFIG_NOSYSTEM"] = "1"
    env["GIT_CONFIG_COUNT"] = "0"
    env["GIT_CONFIG_GLOBAL"] = str(empty_cfg)
    return env


def run(
    command: list[str],
    *,
    cwd: Path,
    env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=cwd,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )


def init_fixture_repository(repo: Path, base_env: dict[str, str] | None = None) -> dict[str, str]:
    repo.mkdir(parents=True, exist_ok=True)
    env = isolated_git_env(repo, base_env)
    init = run(["git", "init", "-q", "--template=", str(repo)], cwd=repo, env=env)
    if init.returncode != 0:
        raise AssertionError(f"git init failed: {init.stderr}")
    top = run(["git", "rev-parse", "--show-toplevel"], cwd=repo, env=env)
    if top.returncode != 0:
        raise AssertionError(f"git rev-parse failed: {top.stderr}")
    actual_root = Path(top.stdout.strip()).resolve()
    expected_root = repo.resolve()
    if actual_root != expected_root:
        raise AssertionError(
            f"Git escaped the fixture repository: expected '{expected_root}', got '{actual_root}'"
        )
    cfg = run(["git", "config", "core.autocrlf", "false"], cwd=repo, env=env)
    if cfg.returncode != 0:
        raise AssertionError(f"git config core.autocrlf failed: {cfg.stderr}")
    return env


def write_executable(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    path.chmod(0o755)
