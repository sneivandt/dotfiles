#!/usr/bin/env python3
"""Select accumulated nightly mutation inputs without inheriting failed coverage."""

import json
import os
from pathlib import Path
import re
import subprocess


def git(*args):
    return subprocess.check_output(["git", *args])


def api_pages(endpoint, key):
    page = 1
    while True:
        result = json.loads(subprocess.check_output(
            ["gh", "api", f"{endpoint}&page={page}"], text=True,
        ))
        yield from result[key]
        if len(result[key]) < 100:
            return
        page += 1


def ancestor(sha, head):
    if not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ValueError("Mutation baseline must be a full commit SHA.")
    result = subprocess.run(["git", "merge-base", "--is-ancestor", sha, head], check=False)
    if result.returncode not in (0, 1):
        raise subprocess.CalledProcessError(result.returncode, result.args)
    return result.returncode == 0


def baseline(repository, head, explicit=""):
    if explicit:
        if not ancestor(explicit, head):
            raise ValueError("Mutation baseline is not an ancestor of the selected commit.")
        return explicit
    runs = api_pages(
        f"repos/{repository}/actions/workflows/mutation.yml/runs"
        "?branch=main&event=schedule&status=success&per_page=100", "workflow_runs",
    )
    for run in runs:
        if ancestor(run["head_sha"], head):
            return run["head_sha"]

    # Migrate the last passing legacy mutation gate, not a newer mutation-free CI.
    runs = api_pages(
        f"repos/{repository}/actions/workflows/ci.yml/runs"
        "?branch=main&event=push&status=success&per_page=100", "workflow_runs",
    )
    expected = {
        *(f"Rust: Mutation (changed code, shard {shard})" for shard in range(8)),
        *(f"Rust: Native Windows mutation (shard {shard})" for shard in range(2)),
    }
    for run in runs:
        if not ancestor(run["head_sha"], head):
            continue
        jobs = api_pages(
            f"repos/{repository}/actions/runs/{int(run['id'])}/jobs?per_page=100", "jobs",
        )
        passed = {job["name"] for job in jobs if job["conclusion"] == "success"}
        if expected <= passed:
            return run["head_sha"]
    return ""


def repository_test_input(path):
    # Config::load in config_drift and the tracked Arch recipe descriptor tests.
    if path.startswith("conf/") and path.endswith(".toml"):
        return True
    if path.startswith("packages/") and path.rsplit("/", 1)[-1] in {"PKGBUILD", "release.json"}:
        return True
    # Repository assets and embedded Markdown examples read by Rust tests.
    return path.startswith((
        "symlinks/config/systemd/user/",
        "symlinks/config/hypr/scripts/",
        "symlinks/local/bin/",
    )) or path in {
        "symlinks/config/Code/User/settings.json",
        "symlinks/config/Thunar/managed-view-settings.sh",
        "symlinks/config/hypr/conf/binds.lua",
        "symlinks/config/shell/path.sh",
        "symlinks/local/share/applications/code-insiders.desktop",
        "symlinks/local/share/applications/code-insiders-url-handler.desktop",
        "docs/content/guides/agent-configuration.md",
        "docs/content/guides/private-overlays.md",
        "docs/content/reference/agent-settings.md",
        "docs/content/reference/cli-reference.md",
        "docs/content/reference/home-symlinks.md",
        "docs/content/reference/system-configuration-files.md",
        "docs/content/reference/systemd-units.md",
        "docs/content/reference/task-reference.md",
    }


def selection(paths):
    source = [path for path in paths if path.startswith("cli/src/") and path.endswith(".rs")]
    broad = any(
        repository_test_input(path)
        or path.startswith(("cli/tests/", "cli/.cargo/", ".cargo/"))
        or path in {
            "cli/Cargo.toml", "cli/Cargo.lock", "cli/build.rs",
            "cli/rust-toolchain", "cli/rust-toolchain.toml", "Cargo.toml", "Cargo.lock", "build.rs",
            "rust-toolchain", "rust-toolchain.toml",
            ".github/workflows/mutation.yml",
            ".github/workflows/scripts/linux/select-mutations.py",
        }
        or (path.startswith("cli/src/") and (
            not path.endswith(".rs") or "/tests/" in path or path.endswith("/tests.rs")
            or "/test_helpers/" in path
            or path.endswith(("_tests.rs", "/test_helpers.rs", "/test_fixture.rs"))
        ))
        for path in paths
    )
    # Whole changed files include unchanged production code beside edited inline
    # tests. Harness, external test, and build changes recheck all source.
    return (["cli/src"] if broad else source), bool(source or broad)


def main():
    head = os.environ["GITHUB_SHA"]
    base = baseline(os.environ["GITHUB_REPOSITORY"], head, os.environ.get("MUTATION_BASE", ""))
    empty_tree = git("hash-object", "-t", "tree", "/dev/null").decode().strip()
    paths = [
        os.fsdecode(path) for path in git(
            "diff", "--name-only", "--no-renames", "-z", base or empty_tree, head, "--",
        ).split(b"\0") if path
    ]
    sources, windows = selection(paths)
    diff = git(
        "diff", "--relative=cli", "--no-renames", "--unified=0",
        empty_tree, head, "--", *sources,
    ) if sources else b""
    Path("cli/mutants.diff").write_bytes(diff)
    outputs = {"run_linux": bool(diff), "run_windows": windows}
    with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as stream:
        for name, value in outputs.items():
            stream.write(f"{name}={str(value).lower()}\n")
    summary = (
        f"Mutation range: {base or 'all source (no passing baseline)'}..{head}\n\n"
        f"Linux scope: {', '.join(sources) or 'none'}\n\n"
        f"Native Windows suite selected: {windows}\n"
    )
    print(summary)
    with Path(os.environ["GITHUB_STEP_SUMMARY"]).open("a", encoding="utf-8") as stream:
        stream.write(summary)


if __name__ == "__main__":
    main()
