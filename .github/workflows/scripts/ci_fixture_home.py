#!/usr/bin/env python3
"""Give CI profiles and application tests a vacant home instead of runner defaults."""

from __future__ import annotations

import os
from pathlib import Path
import secrets
import sys
import tempfile


def _try_mkdir(path: Path) -> bool:
    try:
        path.mkdir()
    except FileExistsError:
        return False
    return True


def create_fixture_home(runner_temp: Path, *, is_windows: bool) -> Path:
    if not is_windows:
        return Path(tempfile.mkdtemp(prefix="dotfiles-home.", dir=runner_temp))

    # Python 3.13+ tempfile.mkdtemp() passes mode 0o700 on Windows, which applies
    # a protected DACL that breaks `dotfiles remove` materialization's ACL check.
    for _ in range(100):
        candidate = runner_temp / f"dotfiles-home.{secrets.token_hex(6)}"
        if _try_mkdir(candidate):
            return candidate
    raise FileExistsError(f"Could not allocate unique fixture home under {runner_temp}")


def main() -> int:
    runner_temp = Path(os.environ["RUNNER_TEMP"])
    github_env = Path(os.environ["GITHUB_ENV"])
    is_windows = os.environ.get("RUNNER_OS") == "Windows" or sys.platform == "win32"
    home = create_fixture_home(runner_temp, is_windows=is_windows)
    config = home / ".config"

    lines = [
        f"HOME={home}",
        f"XDG_CONFIG_HOME={config}",
    ]
    if is_windows:
        lines.insert(0, f"USERPROFILE={home}")

    with github_env.open("a", encoding="utf-8") as stream:
        for line in lines:
            stream.write(f"{line}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
