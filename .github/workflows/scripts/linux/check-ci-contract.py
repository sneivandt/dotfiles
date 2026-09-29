#!/usr/bin/env python3
"""Check CI scheduling and reject accidental skips in the aggregate gate."""

import json
import os
from pathlib import Path
import re
import sys


WORKFLOW = Path(__file__).resolve().parents[2] / "ci.yml"
INFORMATIONAL_JOBS = {"coverage", "mutation"}
JOB_CONDITIONS = {
    "rust-fmt": "run_rust_fmt",
    "lint": "run_lint",
    "managed-script-regressions": "run_managed_scripts",
    "docs": "run_docs_checks",
    "validate-config": "run_validate_config",
    "audit": "run_audit",
    "deny": "run_deny",
    "build-linux": "run_build_linux",
    "msrv": "run_rust_checks",
    "coverage": "run_rust_checks",
    "mutation": "run_mutation",
    "build-windows": "run_build_windows",
    "integration-linux": "run_profile_linux",
    "integration-windows": "run_profile_windows",
    "test-install-uninstall": "run_roundtrip_linux",
    "test-install-uninstall-windows": "run_roundtrip_windows",
    "test-applications": "run_app_tests",
    "test-applications-windows": "run_app_windows",
    "test-git-hooks": "run_git_hooks",
    "test-shell-wrapper-linux": "run_wrapper_linux",
    "test-shell-wrapper-windows": "run_wrapper_windows",
    "test-stocks": "run_stocks",
    "test-hook-inputs": "run_hook_inputs",
}
SPECIAL_CONDITIONS = {
    "validate-config": "needs.classify-changes.outputs.run_validate_config == 'true' && needs.build-linux.result == 'success'",
    "mutation": "needs.classify-changes.outputs.run_mutation == 'true' && (github.event_name == 'pull_request' || (github.event_name == 'push' && github.ref == 'refs/heads/main'))",
}


def job_blocks(workflow: str) -> dict[str, str]:
    jobs = workflow.split("\njobs:\n", 1)
    if len(jobs) != 2:
        raise ValueError("CI workflow has no jobs section")
    headings = list(re.finditer(r"(?m)^  ([A-Za-z_][A-Za-z0-9_-]*):[ \t]*$", jobs[1]))
    if not headings:
        raise ValueError("CI workflow has no jobs")
    return {
        heading.group(1): jobs[1][heading.end() : headings[index + 1].start() if index + 1 < len(headings) else None]
        for index, heading in enumerate(headings)
    }


def gate_dependencies(block: str) -> set[str]:
    match = re.search(
        r"(?m)^    needs:[ \t]*$\n((?:^      - [A-Za-z_][A-Za-z0-9_-]*[ \t]*$\n?)+)",
        block,
    )
    if match is None:
        raise ValueError("ci-success must have a multiline needs list")
    return set(re.findall(r"(?m)^      - ([A-Za-z_][A-Za-z0-9_-]*)[ \t]*$", match.group(1)))


def job_condition(block: str) -> str | None:
    match = re.search(r"(?m)^    if: (.+)$", block)
    return match.group(1).strip() if match else None


def job_needs(block: str) -> set[str]:
    match = re.search(r"(?m)^    needs: \[([^]]+)\]$", block)
    return {name.strip() for name in match.group(1).split(",")} if match else set()


def check(workflow: str) -> None:
    jobs = job_blocks(workflow)
    gate = jobs.get("ci-success")
    if gate is None:
        raise ValueError("missing ci-success job")
    if not re.search(r"(?m)^    if: always\(\)\s*$", gate):
        raise ValueError("ci-success must run under if: always()")
    for job in INFORMATIONAL_JOBS:
        if job not in jobs or not re.search(r"(?m)^    continue-on-error: true\s*$", jobs[job]):
            raise ValueError(f"informational job {job} is missing or no longer non-gating")
    required = jobs.keys() - INFORMATIONAL_JOBS - {"ci-success"}
    actual = gate_dependencies(gate)
    missing = required - actual
    extra = actual - required
    if missing or extra:
        raise ValueError(
            f"ci-success needs mismatch: missing={sorted(missing)}, unexpected={sorted(extra)}"
        )
    expected_conditions = {
        name: f"needs.classify-changes.outputs.{output} == 'true'"
        for name, output in JOB_CONDITIONS.items()
    } | SPECIAL_CONDITIONS
    condition_jobs = jobs.keys() - {"classify-changes", "ci-success"}
    if condition_jobs != expected_conditions.keys():
        raise ValueError(
            "CI condition contract mismatch: "
            f"missing={sorted(condition_jobs - expected_conditions.keys())}, "
            f"unexpected={sorted(expected_conditions.keys() - condition_jobs)}"
        )
    for name, expected in expected_conditions.items():
        actual = job_condition(jobs[name])
        if actual != expected:
            raise ValueError(f"{name} condition changed: expected {expected!r}, got {actual!r}")
        if "classify-changes" not in job_needs(jobs[name]):
            raise ValueError(f"{name} must depend on classify-changes")
        output = JOB_CONDITIONS[name]
        if f"{output}: ${{{{ steps.classify.outputs.{output} }}}}" not in jobs["classify-changes"]:
            raise ValueError(f"classify-changes must expose {output}")

    for name, platform in {
        "validate-config": "linux",
        "integration-linux": "linux",
        "integration-windows": "windows",
        "test-install-uninstall": "linux",
        "test-install-uninstall-windows": "windows",
        "test-applications": "linux",
        "test-applications-windows": "windows",
        "test-shell-wrapper-linux": "linux",
        "test-shell-wrapper-windows": "windows",
    }.items():
        if f"build-{platform}" not in job_needs(jobs[name]):
            raise ValueError(f"{name} must depend on its {platform} artifact")
    for name, output in {"lint": "lint_matrix", "test-applications": "app_matrix"}.items():
        if f"matrix: ${{{{ fromJSON(needs.classify-changes.outputs.{output}) }}}}" not in jobs[name]:
            raise ValueError(f"{name} must use the classified matrix")
    if "check-ci-contract.py --results" not in gate or "CI_NEEDS: ${{ toJSON(needs) }}" not in gate:
        raise ValueError("ci-success must verify selected jobs, not just absence of failures")

    mutation = jobs["mutation"]
    shards = re.search(r"(?m)^        shard: \[([0-9, ]+)\]$", mutation)
    partition = re.search(r'--shard "\$MUTATION_SHARD/([0-9]+)"', mutation)
    if shards is None or partition is None:
        raise ValueError("mutation testing must declare its complete shard partition")
    total = int(partition.group(1))
    indices = [int(index.strip()) for index in shards.group(1).split(",")]
    if total < 1 or sorted(indices) != list(range(total)):
        raise ValueError("mutation shard matrix must cover every shard exactly once")
    if "fail-fast: false" not in mutation or "--cargo-arg=--profile=ci" not in mutation:
        raise ValueError("mutation shards must all run using the ci Cargo profile")


def check_release(workflow: str) -> None:
    jobs = job_blocks(workflow)
    if job_condition(jobs["version"]) != "needs.check-ci.outputs.run_release == 'true'":
        raise ValueError("release version resolution must require changed binary inputs")
    classify = jobs["check-ci"]
    for guard in (
        "github.event.workflow_run.conclusion == 'success'",
        "github.event.workflow_run.event == 'push'",
        "github.event.workflow_run.head_branch == 'main'",
        "github.event.workflow_run.head_repository.full_name == github.repository",
        "ref: ${{ steps.resolve.outputs.sha }}",
        "run_release: ${{ steps.classify.outputs.run_release }}",
        "classify-release.sh",
    ):
        if guard not in classify:
            raise ValueError(f"release classification is missing {guard}")
    for name in ("build-linux", "build-windows", "release"):
        if "version" not in job_needs(jobs[name]):
            raise ValueError(f"release {name} must depend on the selected version")


def check_results(needs: dict) -> None:
    expected_jobs = JOB_CONDITIONS.keys() - INFORMATIONAL_JOBS
    if needs.keys() != expected_jobs | {"classify-changes"}:
        raise ValueError("CI results do not contain exactly the gating jobs")
    classification = needs["classify-changes"]
    if classification["result"] != "success":
        raise ValueError("change classification did not succeed")
    outputs = classification["outputs"]
    for name in sorted(expected_jobs):
        output = JOB_CONDITIONS[name]
        selected = outputs.get(output)
        if selected not in {"true", "false"}:
            raise ValueError(f"missing or invalid classification output: {output}")
        expected = "success" if selected == "true" else "skipped"
        actual = needs[name]["result"]
        if actual != expected:
            raise ValueError(f"{name}: expected {expected}, got {actual}")


if __name__ == "__main__":
    try:
        check(WORKFLOW.read_text(encoding="utf-8"))
        check_release(WORKFLOW.with_name("release.yml").read_text(encoding="utf-8"))
        if sys.argv[1:] == ["--results"]:
            check_results(json.loads(os.environ["CI_NEEDS"]))
    except ValueError as error:
        print(f"CI contract failed: {error}", file=sys.stderr)
        sys.exit(1)
    print("CI job gate and scheduling conditions match the contract")
