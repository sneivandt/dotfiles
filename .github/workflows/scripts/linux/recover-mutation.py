#!/usr/bin/env python3
"""Retry one lost runner, never a mutation finding or an operator-cancelled run."""

import json
import os
import re
import subprocess


def api(path, *options):
    return subprocess.check_output(["gh", "api", *options, path])


def interrupted(job):
    steps = job["steps"]
    return (
        job["run_attempt"] == 1 and job["conclusion"] == "failure"
        and job["name"].startswith(("Rust: Mutation (changed code, shard ", "Rust: Native Windows mutation (shard "))
        and any(s["name"] == "Test mutation partition" and s["conclusion"] == "cancelled" for s in steps)
        and not any(s["conclusion"] == "failure" for s in steps)
        and not any("upload-artifact@" in s["name"] and s["conclusion"] != "skipped" for s in steps)
    )


def lost_runner(log):
    return (b"##[error]The runner has received a shutdown signal." in log
            and not re.search(rb"\b(?:MISSED|TIMEOUT)\b", log))


def recover(repository, run_id):
    actions = f"repos/{repository}/actions"
    run = f"{actions}/runs/{int(run_id)}"
    state = json.loads(api(run))
    if (state["status"] != "completed" or state["conclusion"] != "failure" or state["run_attempt"] != 1
            or state["path"] != ".github/workflows/mutation.yml" or state["head_branch"] != "main"
            or state["head_repository"]["full_name"] != repository
            or state["event"] not in ("schedule", "workflow_dispatch")):
        return
    pages = json.loads(api(f"{run}/attempts/1/jobs?per_page=100", "--paginate", "--slurp"))
    for page in pages:
        for job in page["jobs"]:
            if not interrupted(job):
                continue
            endpoint = f"{actions}/jobs/{int(job['id'])}"
            if lost_runner(api(f"{endpoint}/logs", "--allow-escape-sequences")):
                # GitHub reruns this exact job and its dependents on the original SHA.
                api(f"{endpoint}/rerun", "--method", "POST")
                print(f"Requested one retry for {job['name']} in run {run_id}.")
                return
    print("No confirmed runner interruption to retry.")


if __name__ == "__main__":
    recover(os.environ["GITHUB_REPOSITORY"], os.environ["RUN_ID"])
