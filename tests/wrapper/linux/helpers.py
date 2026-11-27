"""Shared subprocess and fixture helpers for dotfiles.sh wrapper tests."""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess

REPO_ROOT = Path(os.environ.get("DIR") or Path(__file__).resolve().parents[3])
WRAPPER_SRC = REPO_ROOT / "dotfiles.sh"


def isolated_mode() -> bool:
    return os.environ.get("DOTFILES_WRAPPER_ISOLATED") == "1"


def write_executable(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    path.chmod(0o755)


def copy_wrapper(dest_dir: Path) -> Path:
    dest_dir.mkdir(parents=True, exist_ok=True)
    wrapper = dest_dir / "dotfiles.sh"
    shutil.copy2(WRAPPER_SRC, wrapper)
    wrapper.chmod(0o755)
    return wrapper


def symlink_system_tools(bin_dir: Path, tools: tuple[str, ...]) -> None:
    for tool in tools:
        resolved = shutil.which(tool)
        if resolved is None:
            raise AssertionError(f"Required system tool not found on PATH: {tool}")
        target = bin_dir / tool
        if not target.exists():
            target.symlink_to(resolved)


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
