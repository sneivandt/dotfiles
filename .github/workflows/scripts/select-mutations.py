#!/usr/bin/env python3
"""Select accumulated nightly mutation inputs without inheriting failed coverage."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

API_ATTEMPTS = 3


def git(*args, input=None):
    return subprocess.run(["git", *args], input=input, stdout=subprocess.PIPE, check=True).stdout


def gh_api(*args, attempts=API_ATTEMPTS, sleep=time.sleep):
    for attempt in range(1, attempts + 1):
        result = subprocess.run(
            ["gh", "api", *args],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode == 0:
            return result.stdout
        retryable = attempt < attempts and (
            re.search(r"\bHTTP (?:429|5\d\d)\b", result.stderr) or "HTTP 4" not in result.stderr
        )
        if not retryable:
            if result.stderr:
                sys.stderr.write(result.stderr)
            raise subprocess.CalledProcessError(
                result.returncode,
                result.args,
                output=result.stdout,
                stderr=result.stderr,
            )
        sleep(2 ** (attempt - 1))
    raise AssertionError("unreachable")


def api_pages(endpoint, key):
    page = 1
    while True:
        result = json.loads(gh_api(f"{endpoint}&page={page}"))
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
        "?branch=main&event=schedule&status=success&per_page=100",
        "workflow_runs",
    )
    for run in runs:
        if ancestor(run["head_sha"], head):
            return run["head_sha"]

    # Migrate the last passing legacy mutation gate, not a newer mutation-free CI.
    runs = api_pages(
        f"repos/{repository}/actions/workflows/ci.yml/runs"
        "?branch=main&event=push&status=success&per_page=100",
        "workflow_runs",
    )
    expected = {
        *(f"Rust: Mutation (changed code, shard {shard})" for shard in range(8)),
        *(f"Rust: Native Windows mutation (shard {shard})" for shard in range(2)),
    }
    for run in runs:
        if not ancestor(run["head_sha"], head):
            continue
        jobs = api_pages(
            f"repos/{repository}/actions/runs/{int(run['id'])}/jobs?per_page=100",
            "jobs",
        )
        passed = {job["name"] for job in jobs if job["conclusion"] == "success"}
        if expected <= passed:
            return run["head_sha"]
    return ""


REPOSITORY_DOC_OWNERS = {
    "docs/content/guides/agent-configuration.md": ("cli/src/domains/ai/apm/fragments.rs",),
    "docs/content/guides/private-overlays.md": ("cli/src/app/config",),
    "docs/content/reference/agent-settings.md": ("cli/src/domains/ai/resources/agent_settings.rs",),
    "docs/content/reference/cli-reference.md": ("cli/src/app/cli.rs",),
    "docs/content/reference/home-symlinks.md": ("cli/src/domains/files/symlinks.rs",),
    "docs/content/reference/system-configuration-files.md": (
        "cli/src/domains/system/resources/system_file",
        "cli/src/domains/system/resources/system_file.rs",
    ),
    "docs/content/reference/systemd-units.md": (
        "cli/src/domains/system/resources/systemd_unit",
        "cli/src/domains/system/resources/systemd_unit.rs",
    ),
    "docs/content/reference/task-reference.md": ("cli/src/app/commands/list.rs",),
}


def repository_test_owners(path):
    """Map tracked non-Rust files read by Rust tests to their owning modules."""
    if path == "conf/system-files.toml":
        return (
            "cli/src/app/config",
            "cli/src/domains/system/resources/system_file",
            "cli/src/domains/system/resources/system_file.rs",
        )
    if path.startswith("conf/") and path.endswith(".toml"):
        return ("cli/src/app/config",)
    if path.startswith("packages/"):
        parts = path.split("/")
        if len(parts) == 3 and (
            parts[2] in {"PKGBUILD", "release.json"} or parts[2].endswith(".install")
        ):
            return (
                "cli/src/domains/packages/arch_recipes",
                "cli/src/domains/packages/arch_recipes.rs",
            )
    if path.startswith("system/"):
        return (
            "cli/src/domains/system/resources/system_file",
            "cli/src/domains/system/resources/system_file.rs",
        )
    if path.startswith("symlinks/config/git/templates/"):
        return ("cli/src/domains/git",)
    if path.startswith("symlinks/config/systemd/user/"):
        return (
            "cli/src/domains/system/resources/systemd_unit",
            "cli/src/domains/system/resources/systemd_unit.rs",
        )
    return REPOSITORY_DOC_OWNERS.get(path, ())


# Explicit owners for shared fixtures outside cli/src/.
TEST_OWNERS = {
    "cli/tests/fixtures/apm-yaml-fragment.yaml": ("cli/src/infra/yaml.rs",),
    "cli/tests/fixtures/apm-0.33.0-lock.yaml": (
        "cli/src/infra/yaml.rs",
        "cli/src/domains/ai/apm/cowork/legacy_lock.rs",
    ),
}


def unit_test_owners(path):
    """Map a module-local unit test file under cli/src/ to its owning module."""
    if not (path.startswith("cli/src/") and path.endswith(".rs")):
        return ()
    if "/test_helpers/" in path or path.endswith(("/test_helpers.rs", "/test_fixture.rs")):
        return ()
    rel = path.removeprefix("cli/src/")
    if rel.endswith("/tests.rs"):
        module = rel.removesuffix("/tests.rs")
    elif "/tests/" in rel:
        module = rel.split("/tests/", 1)[0]
    elif "/" in rel and (
        rel.rsplit("/", 1)[1].endswith("_tests.rs") or rel.rsplit("/", 1)[1].startswith("tests_")
    ):
        module = rel.rsplit("/", 1)[0]
    else:
        return ()
    if not module:
        return ()
    return (f"cli/src/{module}", f"cli/src/{module}.rs")


LINUX_ONLY_DIRS = (
    "cli/src/domains/packages/arch_recipes",
    "cli/src/domains/system/resources/system_file",
    "cli/src/domains/system/resources/systemd_unit",
)


def linux_only_source(path):
    # Modules compiled or exercised only for Linux domains so strictly Linux
    # source edits do not schedule native Windows mutation shards.
    return (
        path.endswith("/linux.rs")
        or any(
            path == directory or path.startswith(directory + "/") for directory in LINUX_ONLY_DIRS
        )
        or path
        in {
            "cli/src/domains/files/chmod.rs",
            "cli/src/domains/files/config/chmod.rs",
            "cli/src/domains/files/resources/chmod.rs",
            "cli/src/domains/packages/arch_recipes.rs",
            "cli/src/domains/packages/install/paru.rs",
            "cli/src/domains/packages/resources/pacman.rs",
            "cli/src/domains/packages/resources/paru.rs",
            "cli/src/domains/shell/login_shell.rs",
            "cli/src/domains/shell/resources/shell.rs",
            "cli/src/domains/system/config/system_files.rs",
            "cli/src/domains/system/config/systemd_units.rs",
            "cli/src/domains/system/resources/system_file.rs",
            "cli/src/domains/system/resources/systemd_unit.rs",
            "cli/src/domains/system/system_files.rs",
            "cli/src/domains/system/systemd_fixture.rs",
            "cli/src/domains/system/systemd_units.rs",
        }
    )


def selection(paths):
    owners = set()
    remaining = []
    for path in paths:
        if path in TEST_OWNERS:
            owners.update(TEST_OWNERS[path])
        elif unit_owners := unit_test_owners(path):
            owners.update(unit_owners)
        elif repo_owners := repository_test_owners(path):
            owners.update(repo_owners)
        else:
            remaining.append(path)
    source = sorted(
        owners
        | {path for path in remaining if path.startswith("cli/src/") and path.endswith(".rs")}
    )
    broad = any(
        path.startswith(("cli/tests/", "cli/.cargo/", ".cargo/"))
        or path
        in {
            "cli/Cargo.toml",
            "cli/Cargo.lock",
            "cli/build.rs",
            "cli/rust-toolchain",
            "cli/rust-toolchain.toml",
            "Cargo.toml",
            "Cargo.lock",
            "build.rs",
            "rust-toolchain",
            "rust-toolchain.toml",
            ".github/workflows/mutation.yml",
            ".github/workflows/scripts/select-mutations.py",
            ".github/workflows/scripts/run-mutations.py",
        }
        or (
            path.startswith("cli/src/")
            and (
                not path.endswith(".rs")
                or "/tests/" in path
                or "/test_helpers/" in path
                or path.endswith(("/tests.rs", "_tests.rs", "/test_helpers.rs", "/test_fixture.rs"))
            )
        )
        for path in remaining
    )
    run_windows = broad or any(not linux_only_source(path) for path in source)
    # Whole changed files include unchanged production code beside edited inline
    # tests. Harness, integration test, shared helper, and build changes recheck all source.
    return (["cli/src"] if broad else source), run_windows


def select_inputs(base, head):
    empty_tree = git("hash-object", "-t", "tree", "--stdin", input=b"").decode().strip()
    paths = [
        os.fsdecode(path)
        for path in git(
            "diff",
            "--name-only",
            "--no-renames",
            "-z",
            base or empty_tree,
            head,
            "--",
        ).split(b"\0")
        if path
    ]
    sources, windows = selection(paths)
    diff = (
        git(
            "diff",
            "--relative=cli",
            "--no-renames",
            "--unified=0",
            empty_tree,
            head,
            "--",
            *sources,
        )
        if sources
        else b""
    )
    Path("cli/mutants.diff").write_bytes(diff)
    manifest = {"base": base, "head": head, "linux_sources": sources, "run_windows": windows}
    Path("cli/mutation-plan.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )
    return manifest


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    local = parser.add_mutually_exclusive_group()
    local.add_argument(
        "--base", help="Select local replay inputs from this full ancestor SHA to HEAD."
    )
    local.add_argument(
        "--all-source", action="store_true", help="Select all source for local replay."
    )
    args = parser.parse_args(argv)
    local_replay = args.base is not None or args.all_source
    if local_replay:
        head = git("rev-parse", "HEAD").decode().strip()
        base = "" if args.all_source else args.base
        if not args.all_source and not ancestor(base, head):
            parser.error("--base must be an ancestor of HEAD")
    else:
        head = os.environ["GITHUB_SHA"]
        base = baseline(os.environ["GITHUB_REPOSITORY"], head, os.environ.get("MUTATION_BASE", ""))
    manifest = select_inputs(base, head)
    summary = (
        f"Mutation range: {base or 'all source (no passing baseline)'}..{head}\n\n"
        f"Linux scope: {', '.join(manifest['linux_sources']) or 'none'}\n\n"
        f"Native Windows suite selected: {manifest['run_windows']}\n"
    )
    print(summary)
    if not local_replay:
        outputs = {
            "has_linux_inputs": bool(Path("cli/mutants.diff").stat().st_size),
            "run_windows": manifest["run_windows"],
        }
        with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as stream:
            for name, value in outputs.items():
                stream.write(f"{name}={str(value).lower()}\n")
        with Path(os.environ["GITHUB_STEP_SUMMARY"]).open("a", encoding="utf-8") as stream:
            stream.write(summary)


if __name__ == "__main__":
    main()
