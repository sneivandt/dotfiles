#!/usr/bin/env python3
"""Validate committed inputs in an isolated checkout using CI's path policy."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import tomllib


SCRIPTS = Path(__file__).resolve().parent


def plan(selection, msrv, docs_checked=False, windows_checked=False):
    """Return safe local commands and selected coverage reserved for CI."""
    stages = ["ci"] if docs_checked else ["docs", "ci"]
    for flag, stage in (
        ("rust_fmt", "fmt"), ("rust_checks", "clippy"),
        ("rust_checks", "test"), ("rust_checks", "rustdoc"),
        ("validate_config", "config"), ("audit", "audit"), ("deny", "deny"),
        ("desktop_qml", "qml"),
    ):
        if selection["run_" + flag]:
            stages.append(stage)
    for item in selection["lint_matrix"]["include"]:
        if item["name"] == "ShellCheck":
            stages.append("shell")
        elif not windows_checked:
            stages.append("powershell")
    commands = []
    if not docs_checked:
        commands.append(("documentation dependencies", "docs", ["npm", "ci", "--no-audit", "--no-fund"]))
    commands.append(("core checks", ".", ["sh", ".github/workflows/scripts/linux/check.sh", "--strict", *stages]))
    if selection["run_config_drift"] and not selection["run_rust_checks"]:
        commands.append(("configuration drift", "cli", ["cargo", "test", "--locked", "--profile", "ci", "--test", "config_drift"]))
    if selection["run_rust_checks"]:
        if not windows_checked:
            commands.append(("Windows compilation (no runtime coverage)", "cli", ["cargo", "clippy", "--locked", "--profile", "ci", "--target", "x86_64-pc-windows-gnu", "--all-targets", "--", "-D", "warnings"]))
        commands.append(("minimum Rust compilation", "cli", ["rustup", "run", msrv, "cargo", "check", "--locked", "--profile", "ci", "--all-targets"]))
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
    apps = {item["application"] for item in selection["app_matrix"]["include"]}
    if "zsh" in apps:
        commands.append(("isolated shell configuration", ".", ["python3", "-B", scripts + "test-shell-config.py"]))
    if apps & {"vim", "nvim"}:
        commands.append(("isolated editor configuration", ".", ["python3", "-B", "-m", "unittest", "discover", "-s", "symlinks/vim/tests", "-p", "test_*.py"]))
    deferred = [name.removeprefix("run_") for name in (
        "run_build_windows", "run_profile_linux", "run_profile_windows",
        "run_roundtrip_linux", "run_roundtrip_windows", "run_app_tests",
        "run_app_windows", "run_git_hooks", "run_wrapper_linux", "run_wrapper_windows",
        "run_mutation", "run_release",
    ) if selection[name]]
    return commands, deferred


def windows_plan(selection, docs_checked=False):
    """Run native fixtures, leaving platform-independent stages to Linux."""
    commands = []
    if not docs_checked:
        commands.extend([
            ("documentation dependencies", "docs", ["npm.cmd", "ci", "--no-audit", "--no-fund"]),
            ("native documentation", ".", [sys.executable, "-B", "docs/check.py"]),
        ])
    stages = []
    if selection["run_rust_checks"]:
        stages.extend(["clippy", "test"])
    if selection["run_validate_config"]:
        stages.append("config")
    if any(item["name"] == "PSScriptAnalyzer" for item in selection["lint_matrix"]["include"]):
        stages.append("powershell")
    if stages:
        commands.append(("native Windows checks", ".", ["pwsh", "-NoProfile", "-File", ".github/workflows/scripts/windows/Check.ps1", "-Strict", *stages]))
    if selection["run_config_drift"] and not selection["run_rust_checks"]:
        commands.append(("native configuration drift", "cli", ["cargo", "test", "--locked", "--profile", "ci", "--test", "config_drift"]))
    if selection["run_wrapper_windows"]:
        # These functions use fixture executors, not the live installation.
        commands.append(("isolated PowerShell wrapper", ".", ["pwsh", "-NoProfile", "-Command",
            ". ./.github/workflows/scripts/windows/Test-ShellWrapper.ps1; "
            "if (-not (Test-IsolatedWrapperPath)) { exit 1 }; "
            "if (-not (Test-CargoArtifactPath)) { exit 1 }"]))
    for flag, directory in (
        ("lock", "symlinks/config/hypr/scripts/tests"),
        ("desktop_python", "symlinks/config/quickshell/tests/python"),
    ):
        if selection["run_" + flag]:
            commands.append(("native " + flag + " fixtures", ".", [sys.executable, "-B", "-m", "unittest", "discover", "-s", directory, "-p", "test_*.py"]))
    if selection["run_prompt"]:
        commands.append(("native PowerShell prompt", ".", ["pwsh", "-NoProfile", "-File", "symlinks/config/powershell/tests/Test-Prompt.ps1"]))
    return commands


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def comparison_base(root, explicit, head):
    """Recover pending coverage rather than trusting a failed remote tip."""
    if explicit:
        return git(root, "rev-parse", "--verify", explicit + "^{commit}"), False
    tip = git(root, "rev-parse", "--verify", "origin/main^{commit}")
    remote = git(root, "remote", "get-url", "origin")
    match = re.fullmatch(r"(?:https?://|ssh://git@|git@)([^/:]+)[:/]([^/]+/[^/]+?)(?:\.git)?", remote)
    if not match:
        raise RuntimeError("Cannot identify origin's GitHub repository; pass --base with the original verified baseline")
    host, repo = match.groups()

    def runs(*filters):
        command = ["gh", "api", "--hostname", host, "--method", "GET",
                   f"repos/{repo}/actions/workflows/ci.yml/runs",
                   "-f", "event=push", "-f", "branch=main", *filters]
        result = subprocess.check_output(command, cwd=root, text=True)
        return json.loads(result)["workflow_runs"]

    preceding = runs("-f", "head_sha=" + tip, "-F", "per_page=1")
    if preceding and preceding[0]["conclusion"] == "success":
        if subprocess.run(["git", "-C", str(root), "merge-base", "--is-ancestor", tip, head], check=False).returncode == 0:
            return tip, False
    for run in runs("-f", "status=success", "-F", "per_page=100"):
        candidate = run["head_sha"]
        if candidate != head and subprocess.run(
            ["git", "-C", str(root), "merge-base", "--is-ancestor", candidate, head],
            check=False,
        ).returncode == 0:
            return candidate, True
    raise RuntimeError("No successful CI ancestor found; pass --base with the original verified baseline")


def committed_policy(root, head):
    """Load selection rules from the commit being validated, never local edits."""
    path = ".github/workflows/scripts/linux/classify-ci-changes.py"
    namespace = {"__name__": "committed_ci_policy"}
    exec(compile(git(root, "show", head + ":" + path), path, "exec"), namespace)
    return namespace


def snapshot(root, head, destination):
    # A clone preserves executable bits, excludes untracked/unstaged inputs,
    # and supplies real Git metadata needed by configuration/build fixtures.
    subprocess.run(["git", "clone", "--quiet", "--template=", "--shared", "--no-checkout", str(root), str(destination)], check=True)
    hooks = destination.parent / "empty-hooks"
    hooks.mkdir(exist_ok=True)
    options = ["-c", "core.hooksPath=" + str(hooks)]
    if sys.platform == "win32":
        options.extend(["-c", "core.symlinks=false"])
    subprocess.run(["git", "-C", str(destination), *options, "checkout", "--quiet", "--detach", head], check=True)


def target_cache(root):
    if sys.platform == "linux" and (
        os.environ.get("WSL_INTEROP") or os.environ.get("WSL_DISTRO_NAME")
        or re.match(r"^/mnt/[a-z]/", str(root))
    ):
        identity = hashlib.sha256(str(root).encode()).hexdigest()[:16]
        return Path.home() / ".cache/dotfiles-pre-push" / identity / "target"
    return root / "cli/target"


def execute(root, head, commands, selection):
    with tempfile.TemporaryDirectory(prefix="dotfiles-pre-push-", dir="/tmp" if sys.platform == "linux" else None) as temporary:
        temp = Path(temporary)
        checkout = temp / "source"
        snapshot(root, head, checkout)
        env = {key: value for key, value in os.environ.items() if not key.startswith(("GIT_", "DOTFILES_"))}
        env.update(DOTFILES_CHECK_STRICT="1", XDG_STATE_HOME=str(temp / "state"),
                   DOTFILES_LOG_DIR=str(temp / "logs"), CARGO_TARGET_DIR=str(target_cache(root)),
                   RUSTUP_AUTO_INSTALL="0", DIR=str(checkout), DOCS_TEST_PYTHON=sys.executable)
        if sys.platform == "linux":
            tools = []
            if selection["run_lock"]:
                tools.extend(["bash", "jq", "gio"])
                if not (shutil.which("lua") or shutil.which("lua5.4")):
                    raise RuntimeError("required desktop fixture tool unavailable: lua or lua5.4")
            apps = {item["application"] for item in selection["app_matrix"]["include"]}
            if "zsh" in apps:
                tools.extend(["bash", "zsh", "fzf", "zstd", "tmux"])
                if not (shutil.which("bat") or shutil.which("batcat")):
                    raise RuntimeError("required shell fixture tool unavailable: bat or batcat")
            if apps & {"vim", "nvim"}:
                tools.extend(["vim", "nvim"])
            for tool in tools:
                if not shutil.which(tool):
                    raise RuntimeError(f"required fixture tool unavailable: {tool}")
        for label, directory, command in commands:
            print(f"\nRunning {label}", flush=True)
            subprocess.run(command, cwd=checkout / directory, env=env, check=True)


def wsl_command(base, head, full):
    if not shutil.which("wsl"):
        raise RuntimeError("WSL is unavailable; Linux committed validation is required for implementation patches")
    script = subprocess.check_output(
        ["wsl", "--exec", "wslpath", "-u", str(Path(__file__).resolve())], text=True,
    ).strip()
    return ["wsl", "--exec", "python3", "-B", script, "--base", base, "--head", head,
            "--docs-checked", head, "--windows-checked", head, *(["--full"] if full else [])]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", help="explicit original verified baseline (default: verify origin/main's CI, recovering to a successful ancestor)")
    parser.add_argument("--head", default="HEAD", help="commit to validate (default: HEAD)")
    parser.add_argument("--plan", action="store_true", help="print selected checks without running them")
    parser.add_argument("--docs-checked", metavar="COMMIT",
                        help="reuse successful native documentation checks for this exact commit")
    parser.add_argument("--windows-checked", metavar="COMMIT",
                        help="reuse successful native Windows checks for this exact commit; omit cross-target Clippy")
    parser.add_argument("--full", action="store_true", help="select full CI coverage, including during recovery")
    args = parser.parse_args()
    if sys.platform not in {"linux", "win32"}:
        parser.error("this checker requires Linux or Windows with WSL")
    if args.windows_checked and sys.platform != "linux":
        parser.error("--windows-checked is only for the Linux worker")
    root = Path(git(SCRIPTS, "rev-parse", "--show-toplevel"))
    head = git(root, "rev-parse", "--verify", args.head + "^{commit}")
    base, recovery = comparison_base(root, args.base, head)
    for option, value in (("--docs-checked", args.docs_checked), ("--windows-checked", args.windows_checked)):
        if value and git(root, "rev-parse", "--verify", value + "^{commit}") != head:
            raise RuntimeError(option + " must identify the exact commit being validated")
    policy = committed_policy(root, head)
    paths = policy["changed_paths"](str(root), base, head)
    subprocess.run(["git", "-C", str(root), "diff", "--check", base, head, "--"], check=True)
    if not paths:
        print("No changed inputs in the committed range; no checks run.")
        return
    manifest = tomllib.loads(git(root, "show", head + ":cli/Cargo.toml"))
    full = args.full or recovery
    selection = policy["classify"](paths, full=full)
    commands, deferred = plan(selection, manifest["package"]["rust-version"], bool(args.docs_checked), bool(args.windows_checked))
    linux = None
    if sys.platform == "win32":
        commands = windows_plan(selection, bool(args.docs_checked))
        if not selection["docs_only"]:
            linux = wsl_command(base, head, full)
    print(f"Committed range: {base}..{head} ({len(paths)} changed paths)", flush=True)
    if recovery:
        print("Remote tip CI is not successful; selecting full checks from a successful ancestor.", flush=True)
    if args.docs_checked:
        print(f"Reusing successful native documentation checks for {head}; not repeating them on Linux.", flush=True)
    for label, directory, command in commands:
        print(f"LOCAL {label}: (cd {directory} && {shlex.join(command)})", flush=True)
    if linux:
        print("LOCAL Linux checks via WSL: " + subprocess.list2cmdline(linux), flush=True)
    print("REMAINING CI: " + (", ".join(deferred) or "always-on workflow checks"), flush=True)
    print("Mutation testing is separate; see docs/content/testing.md#focused-mutation-testing.\n"
          "Local success does not prove host installation, publishing, or live desktop behavior.", flush=True)
    if sys.platform == "linux" and not args.windows_checked:
        print("Native Windows runtime checks remain unavailable in this Linux-only invocation.", flush=True)
    if args.plan:
        if linux:
            subprocess.run([*linux, "--plan"], check=True)
        return
    execute(root, head, commands, selection)
    if linux:
        subprocess.run(linux, check=True)
    print(f"Selected local checks passed for {head}; remaining CI coverage is listed above.")


if __name__ == "__main__":
    try:
        main()
    except (subprocess.CalledProcessError, OSError, RuntimeError) as error:
        print(f"Pre-push validation failed: {error}", file=sys.stderr)
        sys.exit(1)
