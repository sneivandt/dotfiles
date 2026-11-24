#!/usr/bin/env python3
"""Retry failed mutation runs only when every failed shard is a lost runner."""

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


def superseded(repository, run_id, event):
    runs = json.loads(api(
        f"repos/{repository}/actions/workflows/mutation.yml/runs?branch=main&per_page=20"
    ))["workflow_runs"]
    superseding = {"schedule"} if event == "schedule" else {"schedule", "workflow_dispatch"}
    return any(
        int(other["id"]) > int(run_id)
        and other["event"] in superseding
        and other["conclusion"] != "cancelled"
        for other in runs
    )


def recover(repository, run_id):
    actions = f"repos/{repository}/actions"
    run = f"{actions}/runs/{int(run_id)}"
    state = json.loads(api(run))
    if (state["status"] != "completed" or state["conclusion"] != "failure" or state["run_attempt"] != 1
            or state["path"] != ".github/workflows/mutation.yml" or state["head_branch"] != "main"
            or state["head_repository"]["full_name"] != repository
            or state["event"] not in ("schedule", "workflow_dispatch")):
        return
    if superseded(repository, run_id, state["event"]):
        print(f"Skipping retry for superseded run {run_id}.")
        return
    pages = json.loads(api(f"{run}/attempts/1/jobs?per_page=100", "--paginate", "--slurp"))
    failed = [
        job
        for page in pages
        for job in page["jobs"]
        if job["name"] != "Mutation: Results" and job["conclusion"] not in ("success", "skipped")
    ]
    if not failed:
        print("No failed mutation shards to retry.")
        return
    if not all(interrupted(job) for job in failed):
        print("Skipping retry because non-interrupted failures are present.")
        return
    for job in failed:
        endpoint = f"{actions}/jobs/{int(job['id'])}"
        if not lost_runner(api(f"{endpoint}/logs", "--allow-escape-sequences")):
            print(f"Skipping retry because {job['name']} was not a confirmed runner shutdown.")
            return
    # Rerun every interrupted shard and Mutation: Results together on the original SHA.
    api(f"{run}/rerun-failed-jobs", "--method", "POST")
    names = ", ".join(job["name"] for job in failed)
    print(f"Requested one retry for interrupted shard(s) in run {run_id}: {names}.")


if __name__ == "__main__":
    recover(os.environ["GITHUB_REPOSITORY"], os.environ["RUN_ID"])
