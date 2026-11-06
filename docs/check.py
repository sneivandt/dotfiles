#!/usr/bin/env python3
"""Validate documentation natively, optionally from a committed-only snapshot."""

import argparse
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile


def npm_command():
    name = "npm.cmd" if sys.platform == "win32" else "npm"
    command = shutil.which(name)
    if not command:
        raise RuntimeError(f"{name} is unavailable; install the Node version in docs/.node-version")
    return command


def run(command, cwd, env):
    display = subprocess.list2cmdline(command) if sys.platform == "win32" else shlex.join(command)
    print(f"\n({cwd}) {display}", flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def check(root, env):
    scripts = root / ".github/workflows/scripts/linux"
    skills = root / ".agents/skills/skill-maintenance/scripts"
    npm = npm_command()
    for script, arguments in (
        (skills / "test-check-skills.py", []),
        (skills / "check-skills.py", [str(root)]),
        (scripts / "check-doc-links.py", [str(root)]),
    ):
        run([sys.executable, "-B", str(script), *arguments], root, env)
    for arguments in (["run", "check"], ["test"], ["run", "build"]):
        run([npm, *arguments], root / "docs", env)
    run([sys.executable, "-B", str(scripts / "check-doc-site.py"), str(root)], root, env)
    run([npm, "run", "test:browser"], root / "docs", env)


def git(root, *arguments):
    return subprocess.check_output(["git", "-C", str(root), *arguments])


def documentation_path(path):
    return not path.startswith("cli/src/") and (
        path.endswith(".md") or path.startswith(("docs/", ".agents/"))
    )


def check_committed(root, base, env):
    base_sha = git(root, "rev-parse", "--verify", base + "^{commit}").decode().strip()
    head = git(root, "rev-parse", "--verify", "HEAD^{commit}").decode().strip()
    paths = [
        os.fsdecode(path) for path in
        git(root, "diff", "--name-only", "--no-renames", "-z", base_sha, head, "--").split(b"\0")
        if path
    ]
    if not paths:
        raise RuntimeError("The committed range is empty; no documentation checks ran")
    unsupported = [path for path in paths if not documentation_path(path)]
    if unsupported:
        print("Checking documentation only. Other changes still require implementation checks.", flush=True)
    run(["git", "diff", "--check", base_sha, head, "--"], root, env)
    print(f"Checking committed documentation: {base_sha}..{head}", flush=True)
    with tempfile.TemporaryDirectory(prefix="dotfiles-docs-") as temporary:
        checkout = Path(temporary) / "source"
        hooks = Path(temporary) / "empty-hooks"
        hooks.mkdir()
        run([
            "git", "clone", "--quiet", "--template=", "--shared", "--no-checkout",
            str(root), str(checkout),
        ], root, env)
        run([
            "git", "-c", f"core.hooksPath={hooks}", "-c", "core.symlinks=false",
            "checkout", "--quiet", "--detach", head,
        ], checkout, env)
        run([npm_command(), "ci", "--no-audit", "--no-fund"], checkout / "docs", env)
        run([sys.executable, "-B", str(checkout / "docs/check.py")], checkout, env)
    print(f"Committed documentation checks passed for {head}.", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--committed", action="store_true",
        help="check documentation from HEAD in an isolated native checkout, including mixed patches",
    )
    parser.add_argument("--base", help="comparison base for --committed (default: origin/main)")
    args = parser.parse_args()
    if args.base and not args.committed:
        parser.error("--base requires --committed")
    root = Path(__file__).resolve().parent.parent
    env = {key: value for key, value in os.environ.items() if not key.startswith(("GIT_", "DOTFILES_"))}
    env["DOCS_TEST_PYTHON"] = sys.executable
    if args.committed:
        check_committed(root, args.base or "origin/main", env)
    else:
        check(root, env)
        print("Native documentation checks passed.", flush=True)
    print(
        "Remaining CI coverage: Rust documentation contracts/selectors, workflow checks, and publication. "
        "Browser emulation does not prove native phone behavior.",
        flush=True,
    )


if __name__ == "__main__":
    try:
        main()
    except (subprocess.CalledProcessError, OSError, RuntimeError) as error:
        print(f"Documentation validation failed: {error}", file=sys.stderr)
        sys.exit(1)
