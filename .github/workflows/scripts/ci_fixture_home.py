#!/usr/bin/env python3
"""Give CI profiles and application tests a vacant home instead of runner defaults."""

from __future__ import annotations

import os
from pathlib import Path
import sys
import tempfile


def main() -> int:
    runner_temp = os.environ["RUNNER_TEMP"]
    github_env = Path(os.environ["GITHUB_ENV"])
    home = Path(tempfile.mkdtemp(prefix="dotfiles-home.", dir=runner_temp))
    config = home / ".config"

    lines = [
        f"HOME={home}",
        f"XDG_CONFIG_HOME={config}",
    ]
    if os.environ.get("RUNNER_OS") == "Windows" or sys.platform == "win32":
        lines.insert(0, f"USERPROFILE={home}")

    with github_env.open("a", encoding="utf-8") as stream:
        for line in lines:
            stream.write(f"{line}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
