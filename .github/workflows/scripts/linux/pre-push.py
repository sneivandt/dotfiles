#!/usr/bin/env python3
"""Validate committed inputs in an isolated checkout using CI's path policy."""

import argparse
import os
from pathlib import Path
import runpy
import shlex
import shutil
import subprocess
import sys
import tempfile
import tomllib


SCRIPTS = Path(__file__).resolve().parent
POLICY = runpy.run_path(str(SCRIPTS / "classify-ci-changes.py"))


def plan(selection, msrv):
    """Return safe local commands and selected coverage reserved for CI."""
    stages = ["docs", "ci"]
    for flag, stage in (
        ("rust_fmt", "fmt"), ("rust_checks", "clippy"),
        ("rust_checks", "test"), ("rust_checks", "rustdoc"),
        ("validate_config", "config"), ("audit", "audit"), ("deny", "deny"),
        ("desktop_qml", "qml"),
    ):
        if selection["run_" + flag]:
            stages.append(stage)
    for item in selection["lint_matrix"]["include"]:
        stages.append("shell" if item["name"] == "ShellCheck" else "powershell")
    commands = [("core checks", ".", ["sh", ".github/workflows/scripts/linux/check.sh", "--strict", *stages])]
    if selection["run_config_drift"] and not selection["run_rust_checks"]:
        commands.append(("configuration drift", "cli", ["cargo", "test", "--locked", "--profile", "ci", "--test", "config_drift"]))
    if selection["run_rust_checks"]:
        commands.extend([
            ("Windows compilation (no runtime coverage)", "cli", ["cargo", "clippy", "--locked", "--profile", "ci", "--target", "x86_64-pc-windows-gnu", "--all-targets", "--", "-D", "warnings"]),
            ("minimum Rust compilation", "cli", ["rustup", "run", msrv, "cargo", "check", "--locked", "--profile", "ci", "--all-targets"]),
        ])
    scripts = ".github/workflows/scripts/linux/"
    if selection["run_hook_inputs"] or selection["run_rust_checks"]:
        commands.append(("isolated hook inputs", ".", ["sh", scripts + "test-hook-inputs.sh"]))
    if selection["run_git_hooks"]:
        commands.append(("Git template and cleanup fixtures", ".", ["python3", "-B", scripts + "test-maintenance.py"]))
    if selection["run_wrapper_linux"]:
        commands.append(("isolated wrapper fixtures", ".", ["sh", scripts + "test-shell-wrapper.sh", "test_wrapper_preserves_runtime_context", "test_wrapper_uses_cargo_artifact"]))
    for flag, directory in (
        ("lock", "symlinks/config/hypr/scripts/tests"),
        ("desktop_python", "symlinks/config/quickshell/tests/python"),
    ):
        if selection["run_" + flag]:
            commands.append((flag + " fixtures", ".", ["python3", "-B", "-m", "unittest", "discover", "-s", directory, "-p", "test_*.py"]))
    if selection["run_prompt"]:
        commands.append(("PowerShell prompt fixtures", ".", ["pwsh", "-NoProfile", "-File", "symlinks/config/powershell/tests/Test-Prompt.ps1"]))
    if selection["run_stocks"]:
        commands.append(("stock concurrency fixtures", ".", ["sh", scripts + "test-stocks.sh"]))
    deferred = [name.removeprefix("run_") for name in (
        "run_build_windows", "run_profile_linux", "run_profile_windows",
        "run_roundtrip_linux", "run_roundtrip_windows", "run_app_tests",
        "run_app_windows", "run_git_hooks", "run_wrapper_linux", "run_wrapper_windows",
        "run_mutation", "run_release",
    ) if selection[name]]
    return commands, deferred


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def snapshot(root, head, destination):
    # A clone preserves executable bits, excludes untracked/unstaged inputs,
    # and supplies real Git metadata needed by configuration/build fixtures.
    subprocess.run(["git", "clone", "--quiet", "--template=", "--shared", "--no-checkout", str(root), str(destination)], check=True)
    subprocess.run(["git", "-C", str(destination), "-c", "core.hooksPath=/dev/null", "checkout", "--quiet", "--detach", head], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", default="origin/main", help="remote commit before the push (default: origin/main)")
    parser.add_argument("--head", default="HEAD", help="commit to validate (default: HEAD)")
    parser.add_argument("--plan", action="store_true", help="print selected checks without running them")
    args = parser.parse_args()
    if sys.platform != "linux":
        parser.error("this local checker requires Linux; native Windows coverage remains in CI")
    root = Path(git(SCRIPTS, "rev-parse", "--show-toplevel"))
    base = git(root, "rev-parse", "--verify", args.base + "^{commit}")
    head = git(root, "rev-parse", "--verify", args.head + "^{commit}")
    paths = POLICY["changed_paths"](str(root), base, head)
    subprocess.run(["git", "-C", str(root), "diff", "--check", base, head, "--"], check=True)
    if not paths:
        print("No changed inputs in the committed range; no checks run.")
        return
    manifest = tomllib.loads(git(root, "show", head + ":cli/Cargo.toml"))
    selection = POLICY["classify"](paths)
    commands, deferred = plan(selection, manifest["package"]["rust-version"])
    print(f"Committed range: {base}..{head} ({len(paths)} changed paths)", flush=True)
    for label, directory, command in commands:
        print(f"LOCAL {label}: (cd {directory} && {shlex.join(command)})", flush=True)
    print("REMAINING CI: " + (", ".join(deferred) or "always-on workflow checks"), flush=True)
    print("Mutation testing is separate; see docs/TESTING.md#focused-mutation-testing.\n"
          "Local success does not prove native Windows, host integration, publishing, or live desktop behavior.", flush=True)
    if args.plan:
        return
    with tempfile.TemporaryDirectory(prefix="dotfiles-pre-push-") as temporary:
        temp = Path(temporary)
        checkout = temp / "source"
        snapshot(root, head, checkout)
        env = {key: value for key, value in os.environ.items() if not key.startswith(("GIT_", "DOTFILES_"))}
        env.update(DOTFILES_CHECK_STRICT="1", XDG_STATE_HOME=str(temp / "state"),
                   DOTFILES_LOG_DIR=str(temp / "logs"), CARGO_TARGET_DIR=str(root / "cli/target"),
                   RUSTUP_AUTO_INSTALL="0", DIR=str(checkout))
        # Some desktop fixture suites skip when these tools are absent.
        if selection["run_lock"]:
            for tool in ("bash", "jq", "gio"):
                if not shutil.which(tool):
                    raise RuntimeError(f"required desktop fixture tool unavailable: {tool}")
            if not (shutil.which("lua") or shutil.which("lua5.4")):
                raise RuntimeError("required desktop fixture tool unavailable: lua or lua5.4")
        for label, directory, command in commands:
            print(f"\nRunning {label}", flush=True)
            subprocess.run(command, cwd=checkout / directory, env=env, check=True)
    print(f"Selected local checks passed for {head}; remaining CI coverage is listed above.")


if __name__ == "__main__":
    try:
        main()
    except (subprocess.CalledProcessError, OSError, RuntimeError) as error:
        print(f"Pre-push validation failed: {error}", file=sys.stderr)
        sys.exit(1)
