#!/usr/bin/env python3
"""Summarize mutation reports and require a complete result from each selected shard."""

from datetime import datetime
from html import escape
import json
import os
from pathlib import Path
import shlex
import sys

COUNTS = ("caught", "missed", "timeout", "unviable")
SCRIPT = ".github/workflows/scripts/linux/run-mutations.py"
DISPLAY_LIMIT = 20
WINDOWS_SHARDS = 2


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def code(text):
    return "<code>" + escape(text).replace("|", "&#124;").replace("\n", " ") + "</code>"


def replay(suite, mutation):
    args = [SCRIPT, "run", suite, "--exact", mutation]
    if suite == "windows":
        return "python " + " ".join("'" + value.replace("'", "''") + "'" for value in args)
    return shlex.join(["python3", *args])


def shard_results(directory, expected):
    report = read_json(directory / "outcomes.json")
    duration = (
        (
            datetime.fromisoformat(report["end_time"])
            - datetime.fromisoformat(report["start_time"])
        ).total_seconds()
        if report["end_time"]
        else None
    )
    counts = {key: report[key] for key in COUNTS}
    if any(type(count) is not int or count < 0 for count in counts.values()):
        raise ValueError("invalid mutant counts")
    total = report["total_mutants"]
    problems = []
    if duration is None:
        problems.append("did not complete")
    if sum(counts.values()) != total:
        problems.append("contains unclassified outcomes")
    if (expected is not None and total != expected) or total == 0:
        problems.append(
            f"reported {total} mutants, expected {expected or 'a nonempty native shard'}"
        )
    suites = report.get("suites")
    if suites is not None and not isinstance(suites, dict):
        raise ValueError("invalid suite map")
    names = []
    for key, label in (("missed", "Survivor"), ("timeout", "Timeout")):
        entries = (directory / f"{key}.txt").read_text(encoding="utf-8").splitlines()
        if len(entries) != counts[key]:
            problems.append(f"{key} details disagree with the report")
        names.extend((label, suites.get(entry) if suites else None, entry) for entry in entries)
    return counts, duration, problems, names


def summarize(plan, results, needs):
    base = plan["base"]
    lines = [
        "## Mutation results",
        "",
        f"Range: {code(base or 'all source')} → {code(plan['head'])}",
        "",
        "Linux source selection: " + (", ".join(map(code, plan["linux_sources"])) or "none"),
        "",
        "| Platform | Job | Shards | Caught | Survived | Timed out | Unviable | Longest shard |",
        "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    failures = []
    details = []
    timings = []
    linux = plan["linux_shards"]
    windows_count = plan.get("windows_shards", WINDOWS_SHARDS)
    windows = [
        {"id": f"windows-{index}", "suite": "windows", "count": None}
        for index in range(windows_count)
    ]
    platforms = (
        ("Linux", "mutation", "run_linux", linux),
        ("Windows", "mutation-windows", "run_windows", windows if plan["run_windows"] else []),
    )
    for label, job, flag, shards in platforms:
        selected = needs["select"]["outputs"].get(flag)
        status = needs[job]["result"]
        if selected != str(bool(shards)).lower() or status != ("success" if shards else "skipped"):
            failures.append(f"{label} jobs returned {status} (selected: {selected or 'unknown'}).")
        counts = dict.fromkeys(COUNTS, 0)
        durations = []
        complete = 0
        for shard in shards:
            name = "rust-mutation-results-" + shard["id"]
            try:
                shard_counts, duration, errors, names = shard_results(
                    results / ("mutation-result-" + shard["id"]), shard["count"]
                )
            except (OSError, TypeError, KeyError, ValueError) as error:
                failures.append(f"{name}: {error}")
                continue
            for key, count in shard_counts.items():
                counts[key] += count
            if duration is not None:
                durations.append(duration)
                timings.append((name, sum(shard_counts.values()), duration))
            if errors:
                failures.extend(f"{name}: {error}" for error in errors)
            else:
                complete += 1
            details.extend(
                (outcome, name, replay_suite or shard["suite"], mutation)
                for outcome, replay_suite, mutation in names
            )
        if counts["missed"] or counts["timeout"]:
            failures.append(f"{label} reported survivors or timeouts.")
        totals = " | ".join(str(value) for value in counts.values()) if shards else "— | — | — | —"
        longest = f"{max(durations):.0f}s" if durations else "—"
        lines.append(f"| {label} | {status} | {complete}/{len(shards)} | {totals} | {longest} |")
    if failures:
        lines.extend(["", "### Issues", "", *(f"- {escape(failure)}" for failure in failures)])
    if timings:
        lines.extend(
            [
                "",
                "<details>",
                "<summary>Shard timings</summary>",
                "",
                "| Shard artifact | Mutants reported | Mutation time |",
                "| --- | ---: | ---: |",
                *(f"| {name} | {count} | {duration:.0f}s |" for name, count, duration in timings),
                "",
                "</details>",
            ]
        )
    return lines, details, failures


def render_details(details):
    if not details:
        return []
    return [
        "",
        "### Mutations to replay",
        "",
        "Prepare the same source selection in a disposable checkout using the base above.",
        "Use the tested SHA to reproduce the original mutation before changing its source location.",
        "",
        "| Outcome | Shard artifact | Mutation | Focused command |",
        "| --- | --- | --- | --- |",
        *(
            f"| {outcome} | {shard} | {code(mutation)} | {code(replay(suite, mutation))} |"
            for outcome, shard, suite, mutation in details
        ),
    ]


def main():
    plan_path, results, full_report = map(Path, sys.argv[1:])
    try:
        needs = json.loads(os.environ["MUTATION_NEEDS"])
        if needs["select"]["result"] != "success":
            raise ValueError(f"mutation selection returned {needs['select']['result']}")
        lines, details, failures = summarize(read_json(plan_path), results, needs)
    except (OSError, TypeError, KeyError, ValueError) as error:
        lines, details, failures = ["## Mutation results", "", escape(str(error))], [], [str(error)]
    with Path(os.environ["GITHUB_STEP_SUMMARY"]).open("a", encoding="utf-8") as summary:
        summary.write("\n".join(lines + render_details(details[:DISPLAY_LIMIT])) + "\n")
        if len(details) > DISPLAY_LIMIT:
            summary.write("\nSee the mutation-summary artifact for the complete list.\n")
    full_report.write_text("\n".join(lines + render_details(details)) + "\n", encoding="utf-8")
    return int(bool(failures))


if __name__ == "__main__":
    raise SystemExit(main())
