#!/usr/bin/env python3
"""Validate committed inputs in an isolated checkout using CI's path policy."""

import argparse
from contextlib import ExitStack, contextmanager
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import stat
import subprocess
import sys
import tempfile
import types

SCRIPTS = Path(__file__).resolve().parent


def _load_sibling(name: str):
    if name in sys.modules:
        return sys.modules[name]
    path = SCRIPTS / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise ImportError(f"Cannot load sibling module {name} from {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


GIB = 1024**3
MIN_CACHE_FREE = 10 * GIB


def plan(selection, docs_checked=False, windows_checked=False, linter_stages=None):
    """Return safe local commands and selected coverage reserved for CI."""
    if linter_stages is None:
        linter_stages = _load_sibling("ci_metadata").LINTER_STAGES
    stages = ["ci"] if docs_checked else ["docs", "ci"]
    for flag, stage in (
        ("rust_fmt", "fmt"),
        ("rust_checks", "clippy"),
        ("rust_checks", "test"),
        ("rust_checks", "rustdoc"),
        ("rust_checks", "msrv"),
        ("validate_config", "config"),
        ("audit", "audit"),
        ("deny", "deny"),
        ("desktop_qml", "qml"),
    ):
        if selection["run_" + flag]:
            stages.append(stage)
    for item in selection["lint_matrix"]["include"]:
        stage = linter_stages.get(item["name"])
        if stage is None:
            continue
        if stage == "powershell" and windows_checked:
            continue
        stages.append(stage)
    if selection["run_hook_inputs"] or selection["run_rust_checks"]:
        stages.append("hook-inputs")
    if selection["run_git_hooks"]:
        stages.append("hooks")
    if selection["run_wrapper_linux"]:
        stages.append("wrapper")
    if selection["run_lock"]:
        stages.append("desktop-hypr")
    if selection["run_desktop_python"]:
        stages.append("desktop-python")
    if selection["run_prompt"]:
        stages.append("prompt")
    apps = {item["application"] for item in selection["app_matrix"]["include"]}
    if "zsh" in apps:
        stages.append("shell-config")
    if apps & {"vim", "nvim"}:
        stages.append("vim-config")

    commands = []
    if not docs_checked:
        commands.append(
            ("documentation dependencies", "docs", ["npm", "ci", "--no-audit", "--no-fund"])
        )
    commands.append(
        (
            "core checks",
            ".",
            ["python3", "-B", "tests/check.py", "--strict", *stages],
        )
    )
    if selection["run_config_drift"] and not selection["run_rust_checks"]:
        commands.append(
            (
                "configuration drift",
                "cli",
                ["cargo", "test", "--locked", "--profile", "ci", "--test", "config_drift"],
            )
        )
    if selection["run_rust_checks"] and not windows_checked:
        commands.append(
            (
                "Windows compilation (no runtime coverage)",
                "cli",
                [
                    "cargo",
                    "clippy",
                    "--locked",
                    "--profile",
                    "ci",
                    "--target",
                    "x86_64-pc-windows-gnu",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ],
            )
        )
    deferred = [
        name.removeprefix("run_")
        for name in (
            "run_build_windows",
            "run_profile_linux",
            "run_profile_windows",
            "run_roundtrip_linux",
            "run_roundtrip_windows",
            "run_app_tests",
            "run_app_windows",
            "run_wrapper_linux",
            "run_wrapper_windows",
            "run_release",
            "run_apm",
            "run_apm_windows",
        )
        if selection[name]
    ]
    return commands, deferred


def windows_plan(selection, docs_checked=False):
    """Run native fixtures, leaving platform-independent stages to Linux."""
    commands = []
    if not docs_checked:
        commands.extend(
            [
                (
                    "documentation dependencies",
                    "docs",
                    ["npm.cmd", "ci", "--no-audit", "--no-fund"],
                ),
                ("native documentation", ".", [sys.executable, "-B", "docs/check.py"]),
            ]
        )
    stages = []
    if selection["run_rust_checks"]:
        stages.extend(["clippy", "test"])
    if selection["run_validate_config"]:
        stages.append("config")
    if any(item["name"] == "PSScriptAnalyzer" for item in selection["lint_matrix"]["include"]):
        stages.append("powershell")
    if selection["run_wrapper_windows"]:
        stages.append("wrapper")
    if selection["run_prompt"]:
        stages.append("prompt")
    if stages:
        commands.append(
            (
                "native Windows checks",
                ".",
                [
                    sys.executable,
                    "-B",
                    "tests/check.py",
                    "--strict",
                    *stages,
                ],
            )
        )
    if selection["run_config_drift"] and not selection["run_rust_checks"]:
        commands.append(
            (
                "native configuration drift",
                "cli",
                ["cargo", "test", "--locked", "--profile", "ci", "--test", "config_drift"],
            )
        )
    return commands


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def comparison_base(root, explicit, head):
    """Recover pending coverage rather than trusting a failed remote tip."""
    if explicit:
        return git(root, "rev-parse", "--verify", explicit + "^{commit}"), False
    tip = git(root, "rev-parse", "--verify", "origin/main^{commit}")
    remote = git(root, "remote", "get-url", "origin")
    match = re.fullmatch(
        r"(?:https?://|ssh://git@|git@)([^/:]+)[:/]([^/]+/[^/]+?)(?:\.git)?", remote
    )
    if not match:
        raise RuntimeError(
            "Cannot identify origin's GitHub repository; pass --base with the original verified baseline"
        )
    host, repo = match.groups()

    def runs(*filters):
        command = [
            "gh",
            "api",
            "--hostname",
            host,
            "--method",
            "GET",
            f"repos/{repo}/actions/workflows/ci.yml/runs",
            "-f",
            "event=push",
            "-f",
            "branch=main",
            *filters,
        ]
        result = subprocess.check_output(command, cwd=root, text=True)
        return json.loads(result)["workflow_runs"]

    preceding = runs("-f", "head_sha=" + tip, "-F", "per_page=1")
    if (
        preceding
        and preceding[0]["conclusion"] == "success"
        and subprocess.run(
            ["git", "-C", str(root), "merge-base", "--is-ancestor", tip, head], check=False
        ).returncode
        == 0
    ):
        return tip, False
    for run in runs("-f", "status=success", "-F", "per_page=100"):
        candidate = run["head_sha"]
        if (
            candidate != head
            and subprocess.run(
                ["git", "-C", str(root), "merge-base", "--is-ancestor", candidate, head],
                check=False,
            ).returncode
            == 0
        ):
            return candidate, True
    raise RuntimeError(
        "No successful CI ancestor found; pass --base with the original verified baseline"
    )


def committed_policy(root, head):
    """Load selection rules from the commit being validated, never local edits."""

    def _try_read_blob(candidate):
        try:
            return git(root, "show", head + ":" + candidate)
        except subprocess.CalledProcessError:
            return None

    def _read_first(candidates):
        for candidate in candidates:
            source = _try_read_blob(candidate)
            if source is not None:
                return candidate, source
        return None, None

    meta_path, meta_source = _read_first(
        (
            ".github/workflows/scripts/ci_metadata.py",
            ".github/workflows/scripts/linux/ci_metadata.py",
        )
    )
    policy_path, policy_source = _read_first(
        (
            ".github/workflows/scripts/classify-ci-changes.py",
            ".github/workflows/scripts/linux/classify-ci-changes.py",
        )
    )
    if policy_path is None or policy_source is None:
        raise RuntimeError(f"Could not read classify-ci-changes.py from {head}")
    sentinel = object()
    previous = sys.modules.get("ci_metadata", sentinel)
    try:
        if meta_path is not None and meta_source is not None:
            meta_module = types.ModuleType("ci_metadata")
            exec(compile(meta_source, meta_path, "exec"), meta_module.__dict__)
            sys.modules["ci_metadata"] = meta_module
        namespace = {"__name__": "committed_ci_policy"}
        exec(compile(policy_source, policy_path, "exec"), namespace)
        return namespace
    finally:
        if previous is sentinel:
            sys.modules.pop("ci_metadata", None)
        else:
            sys.modules["ci_metadata"] = previous


def snapshot(root, head, destination):
    # A clone preserves executable bits, excludes untracked/unstaged inputs,
    # and supplies real Git metadata needed by configuration/build fixtures.
    _load_sibling("ci_metadata").snapshot_commit(root, head, destination)


def target_cache(root):
    identity = hashlib.sha256(os.fsencode(os.path.normcase(root.resolve()))).hexdigest()[:16]
    return Path.home() / ".cache/dotfiles-pre-push" / sys.platform / identity / "target"


def free_cache_space(cache):
    location = cache
    while not location.exists():
        location = location.parent
    return shutil.disk_usage(location).free


def cache_directory(cache):
    try:
        info = cache.lstat()
    except FileNotFoundError:
        return False
    junction = sys.platform == "win32" and info.st_reparse_tag == stat.IO_REPARSE_TAG_MOUNT_POINT
    if not stat.S_ISDIR(info.st_mode) or junction or os.path.ismount(cache):
        raise RuntimeError(f"Validation cache must be a regular directory: {cache}")
    return True


def cache_status(cache):
    exists = cache_directory(cache)
    free = free_cache_space(cache)
    size = sum(
        (Path(directory) / name).lstat().st_size
        for directory, _, files in os.walk(cache)
        for name in files
    )
    print(
        f"Validation cache: {cache}\n"
        f"Files: {size / GIB:.2f} GiB. Free: {free / GIB:.2f} GiB. "
        f"Minimum free: {MIN_CACHE_FREE / GIB:.0f} GiB.",
        flush=True,
    )
    if free < MIN_CACHE_FREE:
        action = (
            "Validation would attempt to clear this cache before building."
            if exists
            else "Validation would stop because there is no cache to clear."
        )
        print(action, flush=True)


@contextmanager
def locked_cache(cache):
    cache.parent.mkdir(parents=True, exist_ok=True)
    # Keep the lock outside the directory that may be cleared. File locks are
    # released automatically if the coordinator exits or is interrupted.
    with (cache.parent / "validation.lock").open("ab") as lock:
        try:
            if sys.platform == "win32":
                import msvcrt

                msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl

                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError as error:
            raise RuntimeError(f"Could not lock validation cache {cache}: {error}") from error
        exists = cache_directory(cache)
        free = free_cache_space(cache)
        if free < MIN_CACHE_FREE and exists:
            print(
                f"Clearing validation cache at {cache} with {free / GIB:.2f} GiB free.", flush=True
            )
            shutil.rmtree(cache)
            free = free_cache_space(cache)
        if free < MIN_CACHE_FREE:
            raise RuntimeError(
                f"Only {free / GIB:.2f} GiB free on the filesystem for {cache}. "
                f"Free at least {MIN_CACHE_FREE / GIB:.0f} GiB before running validation."
            )
        yield


def execute(root, head, commands, selection):
    cache = target_cache(root)
    with ExitStack() as stack:
        if any(
            selection["run_" + check]
            for check in ("rust_checks", "validate_config", "config_drift")
        ):
            stack.enter_context(locked_cache(cache))
        temporary = stack.enter_context(
            tempfile.TemporaryDirectory(
                prefix="dotfiles-pre-push-",
                dir="/tmp" if sys.platform == "linux" else None,
            )
        )
        temp = Path(temporary)
        checkout = temp / "source"
        snapshot(root, head, checkout)
        env = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith(("GIT_", "DOTFILES_"))
        }
        env.update(
            DOTFILES_CHECK_STRICT="1",
            XDG_STATE_HOME=str(temp / "state"),
            DOTFILES_LOG_DIR=str(temp / "logs"),
            CARGO_TARGET_DIR=str(cache),
            RUSTUP_AUTO_INSTALL="0",
            DIR=str(checkout),
            DOCS_TEST_PYTHON=sys.executable,
        )
        for label, directory, command in commands:
            print(f"\nRunning {label}", flush=True)
            subprocess.run(command, cwd=checkout / directory, env=env, check=True)


def wsl_command(base, head, full, npm_tls_unavailable=False):
    if not shutil.which("wsl"):
        raise RuntimeError(
            "WSL is unavailable; Linux committed validation is required for implementation patches"
        )
    script = subprocess.check_output(
        ["wsl", "--exec", "wslpath", "-u", str(Path(__file__).resolve())],
        text=True,
    ).strip()
    return [
        "wsl",
        "--exec",
        "python3",
        "-B",
        script,
        "--base",
        base,
        "--head",
        head,
        "--npm-tls-unavailable" if npm_tls_unavailable else "--docs-checked",
        head,
        "--windows-checked",
        head,
        *(["--full"] if full else []),
    ]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--base",
        help="explicit original verified baseline (default: verify origin/main's CI, recovering to a successful ancestor)",
    )
    parser.add_argument("--head", default="HEAD", help="commit to validate (default: HEAD)")
    parser.add_argument(
        "--plan", action="store_true", help="print selected checks without running them"
    )
    parser.add_argument(
        "--cache-status",
        action="store_true",
        help="preview cache cleanup and exit without running checks",
    )
    docs = parser.add_mutually_exclusive_group()
    docs.add_argument(
        "--docs-checked",
        metavar="COMMIT",
        help="reuse successful native documentation checks for this exact commit",
    )
    docs.add_argument(
        "--npm-tls-unavailable",
        metavar="COMMIT",
        help="record a confirmed local npm TLS failure for this exact commit; defer documentation coverage to CI",
    )
    parser.add_argument(
        "--windows-checked",
        metavar="COMMIT",
        help="reuse successful native Windows checks for this exact commit; omit cross-target Clippy",
    )
    parser.add_argument(
        "--full", action="store_true", help="select full CI coverage, including during recovery"
    )
    args = parser.parse_args()
    if sys.platform not in {"linux", "win32"}:
        parser.error("this checker requires Linux or Windows with WSL")
    if args.windows_checked and sys.platform != "linux":
        parser.error("--windows-checked is only for the Linux worker")
    root = Path(git(SCRIPTS, "rev-parse", "--show-toplevel"))
    if args.cache_status:
        cache_status(target_cache(root))
        return
    head = git(root, "rev-parse", "--verify", args.head + "^{commit}")
    base, recovery = comparison_base(root, args.base, head)
    for option, value in (
        ("--docs-checked", args.docs_checked),
        ("--windows-checked", args.windows_checked),
        ("--npm-tls-unavailable", args.npm_tls_unavailable),
    ):
        if value and git(root, "rev-parse", "--verify", value + "^{commit}") != head:
            raise RuntimeError(option + " must identify the exact commit being validated")
    policy = committed_policy(root, head)
    paths = policy["changed_paths"](str(root), base, head)
    subprocess.run(["git", "-C", str(root), "diff", "--check", base, head, "--"], check=True)
    if not paths:
        print("No changed inputs in the committed range; no checks run.")
        return
    full = args.full or recovery
    selection = policy["classify"](paths, full=full)
    omit_docs = bool(args.docs_checked or args.npm_tls_unavailable)
    commands, deferred = plan(
        selection,
        omit_docs,
        bool(args.windows_checked),
        linter_stages=policy.get("LINTER_STAGES"),
    )
    linux = None
    if sys.platform == "win32":
        commands = windows_plan(selection, omit_docs)
        if not selection["docs_only"]:
            linux = wsl_command(
                base, head, full, npm_tls_unavailable=bool(args.npm_tls_unavailable)
            )
    print(f"Committed range: {base}..{head} ({len(paths)} changed paths)", flush=True)
    if recovery:
        print(
            "Remote tip CI is not successful; selecting full checks from a successful ancestor.",
            flush=True,
        )
    if args.docs_checked:
        print(
            f"Reusing successful native documentation checks for {head}; not repeating them on Linux.",
            flush=True,
        )
    if args.npm_tls_unavailable:
        print(
            f"UNAVAILABLE documentation for {head}: confirmed local npm TLS failure; not a passing check. "
            "CI documentation checks must pass before reporting completion.",
            flush=True,
        )
    for label, directory, command in commands:
        print(f"LOCAL {label}: (cd {directory} && {shlex.join(command)})", flush=True)
    if linux:
        print("LOCAL Linux checks via WSL: " + subprocess.list2cmdline(linux), flush=True)
    print("REMAINING CI: " + (", ".join(deferred) or "always-on workflow checks"), flush=True)
    print(
        "Mutation testing is separate; see docs/content/development/testing.md#focused-mutation-testing.\n"
        "Local success does not prove host installation, publishing, or live desktop behavior.",
        flush=True,
    )
    if sys.platform == "linux" and not args.windows_checked:
        print(
            "Native Windows runtime checks remain unavailable in this Linux-only invocation.",
            flush=True,
        )
    if args.plan:
        if linux:
            subprocess.run([*linux, "--plan"], check=True)
        return
    execute(root, head, commands, selection)
    if linux:
        subprocess.run(linux, check=True)
    if args.npm_tls_unavailable:
        print(
            f"Local checks completed for {head} with documentation UNAVAILABLE; "
            "CI documentation coverage is required, alongside the remaining CI checks listed above."
        )
    else:
        print(f"Selected local checks passed for {head}; remaining CI coverage is listed above.")


if __name__ == "__main__":
    try:
        main()
    except (subprocess.CalledProcessError, OSError, RuntimeError) as error:
        print(f"Pre-push validation failed: {error}", file=sys.stderr)
        sys.exit(1)
