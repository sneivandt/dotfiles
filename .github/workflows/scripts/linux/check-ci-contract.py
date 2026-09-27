#!/usr/bin/env python3
"""Check that every gating CI job is included in the aggregate success gate."""

from pathlib import Path
import re
import sys


WORKFLOW = Path(__file__).resolve().parents[2] / "ci.yml"
INFORMATIONAL_JOBS = {"coverage", "mutation"}


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


if __name__ == "__main__":
    try:
        check(WORKFLOW.read_text(encoding="utf-8"))
    except ValueError as error:
        print(f"CI contract failed: {error}", file=sys.stderr)
        sys.exit(1)
    print("CI job gate matches all gating jobs")
