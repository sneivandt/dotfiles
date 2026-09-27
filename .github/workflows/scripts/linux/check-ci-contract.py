#!/usr/bin/env python3
"""Check that every gating CI job is included in the aggregate success gate."""

from pathlib import Path
import re
import sys


WORKFLOW = Path(__file__).resolve().parents[2] / "ci.yml"
INFORMATIONAL_JOBS = {"coverage", "mutation"}
JOB_CONDITIONS = {
    "rust-fmt": "run_rust_checks",
    "lint": "run_lint",
    "docs": "run_docs_checks",
    "audit": "run_rust_checks",
    "deny": "run_rust_checks",
    "build-linux": "run_build_artifacts",
    "msrv": "run_rust_checks",
    "coverage": "run_rust_checks",
    "build-windows": "run_build_artifacts",
    "integration-linux": "run_profile_integration",
    "integration-windows": "run_profile_integration",
    "test-install-uninstall": "run_profile_integration",
    "test-install-uninstall-windows": "run_profile_integration",
    "test-applications": "run_app_tests",
    "test-applications-windows": "run_app_tests",
    "test-git-hooks": "run_git_hooks",
    "test-shell-wrapper-linux": "run_wrapper_linux",
    "test-shell-wrapper-windows": "run_wrapper_windows",
}
SPECIAL_CONDITIONS = {
    "managed-script-regressions": "needs.classify-changes.outputs.run_build_artifacts == 'true' || needs.classify-changes.outputs.run_lint == 'true'",
    "validate-config": "needs.classify-changes.outputs.run_validate_config == 'true' && needs.build-linux.result == 'success'",
    "mutation": "needs.classify-changes.outputs.run_rust_checks == 'true' && (github.event_name == 'pull_request' || (github.event_name == 'push' && github.ref == 'refs/heads/main'))",
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


if __name__ == "__main__":
    try:
        check(WORKFLOW.read_text(encoding="utf-8"))
    except ValueError as error:
        print(f"CI contract failed: {error}", file=sys.stderr)
        sys.exit(1)
    print("CI job gate and scheduling conditions match the contract")
