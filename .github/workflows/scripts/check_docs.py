#!/usr/bin/env python3
"""Documentation link, skill metadata, contract, and site verification runner."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import sys
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Sequence

SCRIPTS_DIR = Path(__file__).resolve().parent
DEFAULT_ROOT = SCRIPTS_DIR.parents[2]


def _run(argv: list[str], *, cwd: Path) -> int:
    try:
        return subprocess.run(argv, cwd=cwd, check=False).returncode
    except OSError as error:
        print(f"ERROR: cannot execute {argv[0]}: {error}", file=sys.stderr)
        return 1


def _run_sequence(commands: list[tuple[list[str], Path]]) -> int:
    for argv, cwd in commands:
        code = _run(argv, cwd=cwd)
        if code != 0:
            return code
    return 0


def plan_commands(
    root: Path,
    checks: Sequence[str],
    *,
    npm: str | None = None,
) -> list[tuple[list[str], Path]]:
    """Return ordered (argv, cwd) pairs for the requested documentation checks."""
    scripts_dir = root / ".github/workflows/scripts"
    skill_scripts = root / ".agents/skills/skill-maintenance/scripts"
    cli_root = root / "cli"
    docs_root = root / "docs"
    npm_cmd = npm or ("npm.cmd" if sys.platform == "win32" else "npm")
    planned: list[tuple[list[str], Path]] = []
    for name in checks:
        if name == "links":
            planned.extend(
                [
                    ([sys.executable, "-B", str(skill_scripts / "test-check-skills.py")], root),
                    (
                        [sys.executable, "-B", str(skill_scripts / "check-skills.py"), str(root)],
                        root,
                    ),
                    (
                        [sys.executable, "-B", str(scripts_dir / "check-doc-links.py"), str(root)],
                        root,
                    ),
                ]
            )
        elif name == "contracts":
            planned.extend(
                [
                    (
                        [
                            "cargo",
                            "test",
                            "--locked",
                            "--quiet",
                            "--profile",
                            "ci",
                            "--lib",
                            "documented_",
                        ],
                        cli_root,
                    ),
                    (
                        [
                            "cargo",
                            "test",
                            "--locked",
                            "--quiet",
                            "--profile",
                            "ci",
                            "--test",
                            "docs_contracts",
                        ],
                        cli_root,
                    ),
                ]
            )
        elif name == "site":
            planned.extend(
                [
                    ([npm_cmd, "run", "check"], docs_root),
                    ([npm_cmd, "test"], docs_root),
                    ([npm_cmd, "run", "build"], docs_root),
                    (
                        [sys.executable, "-B", str(scripts_dir / "check-doc-site.py"), str(root)],
                        root,
                    ),
                    ([npm_cmd, "run", "test:browser"], docs_root),
                ]
            )
        else:
            raise ValueError(f"Unknown documentation check: {name}")
    return planned


def check_links(root: Path) -> int:
    print("== Checking documentation links and repository skill metadata ==", flush=True)
    return _run_sequence(plan_commands(root, ("links",)))


def check_contracts(root: Path) -> int:
    print("== Checking documented task selectors and contracts ==", flush=True)
    return _run_sequence(plan_commands(root, ("contracts",)))


def check_site(root: Path) -> int:
    print("== Checking the built documentation and browser behavior ==", flush=True)
    return _run_sequence(plan_commands(root, ("site",)))


CHECKS = {
    "links": check_links,
    "contracts": check_contracts,
    "site": check_site,
}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        default=os.environ.get("DIR") or str(DEFAULT_ROOT),
        help="Repository root directory",
    )
    parser.add_argument("checks", nargs="+", choices=tuple(CHECKS))
    args = parser.parse_args(argv)
    root = Path(args.root).resolve()

    for name in args.checks:
        code = CHECKS[name](root)
        if code != 0:
            return code
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
