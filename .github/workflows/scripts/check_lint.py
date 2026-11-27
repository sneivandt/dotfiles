#!/usr/bin/env python3
"""Cross-platform static-analysis and workflow lint runners used by CI and tests/check.py."""

from __future__ import annotations

import argparse
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys

SCRIPTS_DIR = Path(__file__).resolve().parent
DEFAULT_ROOT = SCRIPTS_DIR.parents[2]


def _load_ci_metadata():
    if "ci_metadata" in sys.modules:
        return sys.modules["ci_metadata"]
    path = SCRIPTS_DIR / "ci_metadata.py"
    spec = importlib.util.spec_from_file_location("ci_metadata", path)
    if spec is None or spec.loader is None:
        raise ImportError(f"Cannot load ci_metadata from {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _run(argv: list[str], *, cwd: Path) -> int:
    try:
        return subprocess.run(argv, cwd=cwd, check=False).returncode
    except OSError as error:
        print(f"ERROR: cannot execute {argv[0]}: {error}", file=sys.stderr)
        return 1


def _resolve_script(root: Path, rel: str) -> Path:
    candidate = root / rel
    return candidate if candidate.is_file() else DEFAULT_ROOT / rel


def check_shell(root: Path) -> int:
    script = _resolve_script(root, "cli/src/app/validation/scripts/shellcheck.sh")
    return _run(["sh", str(script), "--root", str(root)], cwd=root)


def check_powershell(root: Path) -> int:
    if shutil.which("pwsh") is None:
        print("ERROR: pwsh is not installed; cannot run PSScriptAnalyzer", file=sys.stderr)
        return 1
    script = _resolve_script(root, "cli/src/app/validation/scripts/psscriptanalyzer.ps1")
    return _run(["pwsh", "-NoProfile", "-File", str(script), "-Root", str(root)], cwd=root)


def check_python(root: Path) -> int:
    meta = _load_ci_metadata()
    targets = meta.ruff_inputs(root)
    config = str(root / ".github/linters/ruff.toml")
    status = _run(["ruff", "check", "--config", config, "--", *targets], cwd=root)
    fmt_status = _run(["ruff", "format", "--check", "--config", config, "--", *targets], cwd=root)
    return status or fmt_status


def check_lua(root: Path) -> int:
    files: list[str] = []
    for base in ("symlinks", "tests"):
        directory = root / base
        if directory.is_dir():
            files.extend(str(path) for path in sorted(directory.rglob("*.lua")) if path.is_file())
    if not files:
        return 0
    config = str(root / ".github/linters/luacheckrc")
    return _run(["luacheck", "--config", config, "--", *files], cwd=root)


def check_structured(root: Path) -> int:
    script = SCRIPTS_DIR / "check-structured-files.py"
    return _run([sys.executable, "-B", str(script), "--root", str(root)], cwd=root)


def check_workflows(root: Path) -> int:
    workflows = sorted(str(path) for path in (root / ".github/workflows").glob("*.yml"))
    if not workflows:
        return 0
    # Embedded shell lint has separate coverage; check workflow syntax, expressions,
    # action inputs, and contexts without depending on optional embedded linters.
    return _run(["actionlint", "-shellcheck=", "-pyflakes=", *workflows], cwd=root)


CHECKS = {
    "shell": check_shell,
    "powershell": check_powershell,
    "python": check_python,
    "lua": check_lua,
    "structured": check_structured,
    "workflows": check_workflows,
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
