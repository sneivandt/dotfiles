#!/usr/bin/env python3
"""Documentation link, skill metadata, contract, and site verification runner."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import sys

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


def check_links(root: Path) -> int:
    print("== Checking documentation links and repository skill metadata ==", flush=True)
    skill_scripts = root / ".agents/skills/skill-maintenance/scripts"
    return _run_sequence(
        [
            ([sys.executable, "-B", str(skill_scripts / "test-check-skills.py")], root),
            ([sys.executable, "-B", str(skill_scripts / "check-skills.py"), str(root)], root),
            ([sys.executable, "-B", str(SCRIPTS_DIR / "check-doc-links.py"), str(root)], root),
        ]
    )


def check_contracts(root: Path) -> int:
    print("== Checking documented task selectors and contracts ==", flush=True)
    cli_root = root / "cli"
    return _run_sequence(
        [
            (
                ["cargo", "test", "--locked", "--quiet", "--profile", "ci", "--lib", "documented_"],
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


def check_site(root: Path) -> int:
    print("== Checking the built documentation and browser behavior ==", flush=True)
    docs_root = root / "docs"
    npm = "npm.cmd" if sys.platform == "win32" else "npm"
    return _run_sequence(
        [
            ([npm, "run", "check"], docs_root),
            ([npm, "test"], docs_root),
            ([npm, "run", "build"], docs_root),
            ([sys.executable, "-B", str(SCRIPTS_DIR / "check-doc-site.py"), str(root)], root),
            ([npm, "run", "test:browser"], docs_root),
        ]
    )


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
