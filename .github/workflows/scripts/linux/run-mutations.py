#!/usr/bin/env python3
"""Plan and run the same mutation suites in Actions and in a disposable checkout."""

import argparse
from datetime import datetime
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

CLI = Path(__file__).resolve().parents[4] / "cli"
PLAN = CLI / "mutation-plan.json"
DIFF = "mutants.diff"
LIB_SHARD_TARGET = 300
MAX_LIB_SHARDS = 13
APP_SHARD_TARGET = 250
MAX_APP_SHARDS = 3
WINDOWS_SHARDS = 2
COUNTS = ("caught", "missed", "timeout", "unviable")
APP_PATHS = ("src/main.rs", "src/lib.rs", "src/app/**", "src/testing.rs")

# Mutants that can block unrelated tests get a bounded test target.
FOCUSED = {
    "liveness": (
        (
            "src/engine/access.rs",
            "src/infra/atomic_flag.rs",
            "src/infra/cancellation.rs",
        ),
        r"^src/(engine/access\.rs|infra/(atomic_flag|cancellation)\.rs):[0-9]+:[0-9]+: replace (AccessRegistry::try_acquire -> Option<AccessGuard> with None|AtomicFlag::set with \(\)|AtomicFlag::get -> bool with false|CancellationToken::cancel with \(\)|CancellationToken::is_cancelled -> bool with false)$",
        "engine::access::tests::",
    ),
    "context": (
        ("src/engine/context.rs",),
        r"^src/engine/context\.rs:[0-9]+:[0-9]+: (delete ! in Context::scoped_access|replace Context::try_access -> Option<AccessGuard> with None|replace Context::cancellation_token -> CancellationToken with Default::default\(\))$",
        "engine::context::tests::",
    ),
    "process": (
        ("src/infra/exec/mod.rs", "src/infra/exec/process.rs"),
        r"^src/infra/exec/(mod|process)\.rs:[0-9]+:[0-9]+: replace (\|\| with && in collect_child|wait_for_pipe_close -> bool with false|child_exited_without_reaping -> nix::Result<bool> with Ok\(false\))$",
        "infra::exec::tests::interruption::bounded_command_wait_",
    ),
    "dispatch": (
        ("src/engine/orchestrate.rs",),
        r"^src/engine/orchestrate\.rs:[0-9]+:[0-9]+: delete ! in process_items$",
        "engine::tests::orchestrate::pipelines_accumulate_mixed_states_in_both_execution_modes",
    ),
    "utc": (
        ("src/infra/logging/utils.rs",),
        r"^src/infra/logging/utils\.rs:[0-9]+:[0-9]+: replace .* in civil_from_epoch_secs$",
        "infra::logging::utils::tests::unix_seconds_convert_to_utc_across_day_and_calendar_boundaries",
    ),
}

# These entire modules are compiled only on Windows. Use the same list on
# both platforms so a Linux exclusion cannot accidentally remove coverage.
NATIVE_MODULES = (
    "src/infra/fs/mutation/windows.rs",
    "src/infra/fs/place/windows.rs",
    "src/infra/fs/place/windows/private_dir.rs",
    "src/infra/run_lock/coordination/windows.rs",
    "src/infra/run_lock/windows.rs",
)

LINUX_EXCLUSIONS = (
    *(("--exclude", path) for path in NATIVE_MODULES),
    (
        "--exclude-re",
        r"^src/infra/fs/mod\.rs:[0-9]+:[0-9]+: replace (!= with ==|& with [|^]) in is_dir_like$",
    ),
    (
        "--exclude-re",
        r"^src/infra/fs/copy\.rs:.*\b(verify_link_creation_access|is_windows_reparse_link|preserve_link_attributes)\b",
    ),
    (
        "--exclude-re",
        r"^src/infra/fs/copy\.rs:[0-9]+:[0-9]+: replace < with <= in copy_regular_contents$",
    ),
    (
        "--exclude-re",
        r"^src/infra/fs/identity\.rs:.*\b(stored_entry_name|is_windows_reparse_point)\b",
    ),
    (
        "--exclude-re",
        r"^src/infra/fs/planned\.rs:.*\b(directory_case_sensitive|ordinal_alias|is_within_windows_alias|trees_overlap_non_linux)\b",
    ),
    (
        "--exclude-re",
        r"^src/infra/fs/mutation\.rs:[0-9]+:[0-9]+: replace \| with \^ in open_in_place$",
    ),
    (
        "--exclude-re",
        r"^src/infra/fs/place\.rs:.*\b(rename_noreplace_windows|rename_noreplace_unsupported|non_linux_file_mode)\b",
    ),
    ("--exclude-re", r"^src/infra/fs/place\.rs:[0-9]+:[0-9]+: replace \| with \^$"),
    (
        "--exclude-re",
        r"^src/infra/exec/process\.rs:.*\b(take_stdout_windows|take_stderr_windows|terminate_child_windows|wait_after_terminate_windows|force_kill_child_windows|child_exited_without_reaping_fallback)\b",
    ),
    (
        "--exclude-re",
        r"^src/infra/platform\.rs:.*\b(developer_mode_flag_windows|developer_mode_enabled|detect_arch|detect_wsl)\b",
    ),
    (
        "--exclude-re",
        r"^src/infra/elevation\.rs:.*\b(run_elevated_child|wait_if_elevated|is_elevated_windows|is_elevated_child|sudo_credentials_cached|prime_sudo_credentials)\b",
    ),
    ("--exclude-re", r"^src/infra/logging/runlog/naming\.rs:[0-9]+:[0-9]+: replace \| with \^$"),
    ("--exclude-re", r"^src/infra/logging/subscriber\.rs:.*\binit_subscriber\b"),
    (
        "--exclude-re",
        r"^src/infra/logging/terminal\.rs:.*\b(columns|stream_columns_windows|enable_console_ansi)\b",
    ),
    (
        "--exclude-re",
        r"^src/infra/logging/style\.rs:.*\b(no_color_enabled|clicolor_force_enabled)\b",
    ),
    ("--exclude-re", r"^src/infra/logging/utils\.rs:.*\bdotfiles_log_dir\b"),
    (
        "--exclude-re",
        r"^src/app/commands/execution/elevation\.rs:.*\b(prepare_windows_elevation|prepare_elevation_fallback)\b",
    ),
    ("--exclude-re", r"^src/app/interrupt\.rs:.*\bend_prompt_line\b"),
    ("--exclude-re", r"^src/engine/context\.rs:.*\bis_elevated\b"),
    (
        "--exclude-re",
        r"^src/engine/orchestrate\.rs:[0-9]+:[0-9]+: replace > with >= in process_items$",
    ),
    ("--exclude-re", r"^src/engine/task/mod\.rs:.*\b(dependencies|ordering_dependencies)\b"),
    (
        "--exclude-re",
        r"^src/domains/system/config/registry\.rs:.*\buppercase_native_registry_name\b",
    ),
    (
        "--exclude-re",
        r"^src/domains/dotfiles/resources/path_entry\.rs:.*\b(decode_reg_string|encode_reg_string|append_user_path|user_path_contains|broadcast_environment_change)\b",
    ),
    ("--exclude-re", r"^src/app/commands/reexec\.rs:.*\bprepare_self_update\b"),
    ("--exclude-re", r"^src/domains/dotfiles/self_update\.rs:.*\bpre_update\b"),
    (
        "--exclude-re",
        r"^src/domains/dotfiles/self_update/attestation\.rs:.*\b(available|retry_delay)\b",
    ),
    ("--exclude-re", r"^src/domains/dotfiles/self_update/http\.rs:.*\bretry_delay\b"),
    (
        "--exclude-re",
        r"^src/domains/dotfiles/self_update/install\.rs:.*\bis_windows_transient_busy\b",
    ),
    (
        "--exclude-re",
        r"^src/domains/dotfiles/self_update/install\.rs:[0-9]+:[0-9]+: replace <impl Drop for UpdateJournal>::drop with \(\)$",
    ),
    (
        "--exclude-re",
        r"^src/domains/editors/vscode_extensions/launcher\.rs:.*\b(managed_insiders_unavailable|managed_insiders_unavailable_non_linux)\b",
    ),
    ("--exclude-re", r"^src/domains/overlay/resolution\.rs:.*\bread_worktree_confirmation\b"),
    ("--exclude-re", r"^src/domains/repository/update/apply\.rs:.*\bfetch_retry_delay\b"),
    ("--exclude-re", r"^src/domains/packages/resources/pacman\.rs:.*\bis_root\b"),
    ("--exclude-re", r"^src/domains/system/resources/registry\.rs:.*\bnative::"),
    (
        "--exclude-re",
        r"^src/domains/system/resources/systemd_unit/offline\.rs:[0-9]+:[0-9]+: replace match guard error\.kind\(\) == std::io::ErrorKind::NotFound with true in SystemdUnitResource::reconcile_user_links$",
    ),
    (
        "--exclude-re",
        r"^src/domains/ai/resources/agent_settings\.rs:[0-9]+:[0-9]+: replace match guard metadata\.is_symlink\(\) with true in <impl Resource for AgentSettingResource>::apply$",
    ),
    (
        "--exclude-re",
        r"^src/domains/ai/apm/cowork/skill_files\.rs:[0-9]+:[0-9]+: delete match arm \(false, _\) in skill_files_match$",
    ),
    (
        "--exclude-re",
        r"^src/domains/ai/apm/cowork/skill_files\.rs:[0-9]+:[0-9]+: (delete match arm \(false, _\)|replace (?:match guard )?is_dir_like\(meta\) with (true|false)) in copy_skill_files$",
    ),
    (
        "--exclude-re",
        r"^src/domains/files/resources/symlink/platform\.rs:.*\b(create_symlink_windows|create_junction|command_output)\b",
    ),
    (
        "--exclude-re",
        r"^src/domains/files/resources/symlink/platform\.rs:[0-9]+:[0-9]+: replace (!= with ==|& with [|^]|\|\| with &&|&& with \|\|) in is_link_like$",
    ),
)

WINDOWS_EXCLUSIONS = (
    (
        "--exclude-re",
        r"^src/infra/fs/place/windows\.rs:[0-9]+:[0-9]+: replace <impl Drop for LocalDescriptor>::drop with \(\)$",
    ),
    ("--exclude-re", r"^src/infra/fs/place/windows\.rs:[0-9]+:[0-9]+: replace \| with \^$"),
    (
        "--exclude-re",
        r"^src/infra/fs/place/windows\.rs:.*\b(require_query_support|require_persistent_acls)\b",
    ),
    (
        "--exclude-re",
        r"^src/infra/fs/place/windows/private_dir\.rs:[0-9]+:[0-9]+: replace \| with \^$",
    ),
    (
        "--exclude-re",
        r"^src/infra/run_lock/coordination/windows\.rs:[0-9]+:[0-9]+: replace <impl Drop for KnownFolder>::drop with \(\)$",
    ),
)

WINDOWS_FILES = (
    *NATIVE_MODULES,
    "src/domains/dotfiles/resources/path_entry.rs",
    "src/domains/system/resources/registry.rs",
    "src/infra/elevation.rs",
    "src/app/commands/execution/elevation.rs",
    "src/infra/fs/place.rs",
    "src/infra/fs/mod.rs",
    "src/infra/exec/process.rs",
    "src/domains/system/config/registry.rs",
)
WINDOWS_PATTERN = r"(decode_reg_string|encode_reg_string|native::raw_value_to_string|native::raw_value_type|run_elevated_child|prepare_windows_elevation|rename_noreplace_windows|is_dir_like|uppercase_native_registry_name|take_stdout_windows)"
WINDOWS_PATTERN += "|^(?:" + "|".join(re.escape(path) for path in NATIVE_MODULES) + "):"


def resolve_test_tool(requested="auto"):
    if requested == "auto":
        return "nextest" if shutil.which("cargo-nextest") else "cargo"
    return requested


def suite_options(suite, exact=None, test_tool="cargo"):
    nextest = test_tool == "nextest"
    quiet_nextest = ("--status-level=none", "--final-status-level=fail")
    if suite in ("linux", "linux-lib", "linux-app"):
        excludes = [
            *LINUX_EXCLUSIONS,
            *(("--exclude-re", pattern) for _, pattern, _ in FOCUSED.values()),
        ]
        scope = []
        if suite == "linux-lib":
            scope = [arg for path in APP_PATHS for arg in ("--exclude", path)]
        elif suite == "linux-app":
            scope = [arg for path in APP_PATHS for arg in ("--file", path)]
        options = ["--in-diff", DIFF, *scope, *(arg for pair in excludes for arg in pair)]
        if suite == "linux-lib":
            test_target = (
                ["--cargo-arg=--lib", "--", *quiet_nextest] if nextest else ["--cargo-arg=--lib"]
            )
        else:
            test_target = (
                ["--", "-E", "not binary(domain_boundaries)", *quiet_nextest] if nextest else []
            )
        pattern = None
    elif suite == "windows":
        options = [
            *(arg for path in WINDOWS_FILES for arg in ("--file", path)),
            *(arg for pair in WINDOWS_EXCLUSIONS for arg in pair),
        ]
        test_target = (
            ["--cargo-arg=--lib", "--", *quiet_nextest] if nextest else ["--cargo-arg=--lib"]
        )
        pattern = WINDOWS_PATTERN
    else:
        files, pattern, test = FOCUSED[suite]
        options = ["--in-diff", DIFF, *(arg for path in files for arg in ("--file", path))]
        test_target = (
            ["--cargo-arg=--lib", "--", *quiet_nextest, test]
            if nextest
            else ["--cargo-arg=--lib", "--", test]
        )
    if exact is not None:
        # cargo-mutants combines multiple --re arguments with OR.
        pattern = "^" + re.sub(r"([\\.^$|?*+(){}\[\]])", r"\\\1", exact) + "$"
    if pattern is not None:
        options.extend(("--re", pattern))
    return options, test_target


def cargo_command(suite, exact=None):
    options, _ = suite_options(suite, exact)
    return ["cargo", "mutants", "--no-shuffle", *options]


def build_run_command(suite, test_tool="cargo", partition="", exact=None):
    options, test_target = suite_options(suite, exact, test_tool=test_tool)
    command = ["cargo", "mutants", "--no-shuffle", *options]
    if partition:
        command.extend(("--shard", partition, "--sharding=round-robin"))
    command.extend(("--in-place", "--profile=mutants", "--cargo-arg=--locked"))
    if test_tool == "nextest":
        command.append("--test-tool=nextest")
    command.extend(test_target)
    return command


def mutation_env():
    env = dict(os.environ)
    # Swatinem/rust-cache exports CARGO_INCREMENTAL=0; re-enable incremental
    # compilation so [profile.mutants] keeps unchanged codegen units warm.
    env["CARGO_INCREMENTAL"] = "1"
    # Pin DOTFILES_VERSION so build.rs skips git probes and stays clean across
    # in-place mutations instead of flipping to -dirty and invalidating builds.
    env.setdefault("DOTFILES_VERSION", "dev-0.1.0")
    return env


def list_mutants(suite):
    result = subprocess.check_output([*cargo_command(suite), "--list"], cwd=CLI, encoding="utf-8")
    return result.splitlines()


def round_robin_shards(prefix, suite, total, target, max_shards):
    if not total:
        return []
    partitions = min(max_shards, (total + target - 1) // target)
    return [
        {
            "id": f"{prefix}-{index}",
            "suite": suite,
            "partition": f"{index}/{partitions}",
            "count": (total - index + partitions - 1) // partitions,
        }
        for index in range(partitions)
    ]


def linux_matrix(counts):
    shards = []
    focused_total = sum(counts[suite] for suite in FOCUSED)
    if focused_total:
        # Schedule focused first so it finishes quickly and frees a concurrency
        # slot when a full baseline spans all 15 shards under max-parallel: 14.
        shards.append(
            {
                "id": "focused",
                "suite": "focused",
                "partition": "",
                "count": focused_total,
            }
        )
    shards.extend(
        round_robin_shards(
            "app",
            "linux-app",
            counts.get("linux-app", 0),
            APP_SHARD_TARGET,
            MAX_APP_SHARDS,
        )
    )
    shards.extend(
        round_robin_shards(
            "lib",
            "linux-lib",
            counts.get("linux-lib", 0),
            LIB_SHARD_TARGET,
            MAX_LIB_SHARDS,
        )
    )
    return shards


def plan():
    manifest = json.loads(PLAN.read_text(encoding="utf-8"))
    counts = dict.fromkeys(("linux-lib", "linux-app", *FOCUSED), 0)
    if (CLI / DIFF).stat().st_size:
        counts = {suite: len(list_mutants(suite)) for suite in counts}
    manifest["linux_shards"] = linux_matrix(counts)
    manifest["windows_shards"] = WINDOWS_SHARDS
    PLAN.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as output:
        output.write(f"run_linux={str(bool(manifest['linux_shards'])).lower()}\n")
        output.write("linux_matrix=" + json.dumps({"include": manifest["linux_shards"]}) + "\n")


def merge_focused_reports(stage_dir, selected, output_dir):
    if output_dir.exists():
        shutil.rmtree(output_dir)
    output_dir.mkdir(parents=True)
    counts = dict.fromkeys(COUNTS, 0)
    total_mutants = 0
    outcomes = []
    missed_entries = []
    timeout_entries = []
    suites = {}
    starts = []
    ends = []
    incomplete = False
    for sub_suite in selected:
        src = stage_dir / sub_suite
        if not src.is_dir():
            incomplete = True
            continue
        dst = output_dir / sub_suite
        shutil.move(str(src), str(dst))
        try:
            report = json.loads((dst / "outcomes.json").read_text(encoding="utf-8"))
            for key in COUNTS:
                counts[key] += int(report[key])
            total_mutants += int(report["total_mutants"])
            if isinstance(report.get("outcomes"), list):
                outcomes.extend(report["outcomes"])
            if report.get("start_time"):
                starts.append(report["start_time"])
            if report.get("end_time"):
                ends.append(report["end_time"])
            else:
                incomplete = True
            for filename, bucket in (
                ("missed.txt", missed_entries),
                ("timeout.txt", timeout_entries),
            ):
                entries = (dst / filename).read_text(encoding="utf-8").splitlines()
                bucket.extend(entries)
                for entry in entries:
                    suites[entry] = sub_suite
        except (OSError, KeyError, TypeError, ValueError):
            incomplete = True
    start_time = min(starts, key=datetime.fromisoformat) if starts else None
    end_time = None if (incomplete or not ends) else max(ends, key=datetime.fromisoformat)
    merged = {
        "start_time": start_time,
        "end_time": end_time,
        "total_mutants": total_mutants,
        **counts,
        "outcomes": outcomes,
        "suites": suites,
    }
    (output_dir / "outcomes.json").write_text(json.dumps(merged, indent=2) + "\n", encoding="utf-8")
    (output_dir / "missed.txt").write_text(
        "".join(f"{entry}\n" for entry in missed_entries),
        encoding="utf-8",
    )
    (output_dir / "timeout.txt").write_text(
        "".join(f"{entry}\n" for entry in timeout_entries),
        encoding="utf-8",
    )
    return not incomplete


def run_focused(test_tool):
    selected = [suite for suite in FOCUSED if list_mutants(suite)]
    if not selected:
        raise ValueError("No focused mutants selected.")
    env = mutation_env()
    out_dir = CLI / "mutants.out"
    exit_code = 0
    with tempfile.TemporaryDirectory(dir=CLI, prefix=".mutants-focused-") as temp:
        stage_dir = Path(temp)
        for sub_suite in selected:
            if out_dir.exists():
                shutil.rmtree(out_dir)
            command = build_run_command(sub_suite, test_tool=test_tool)
            status = subprocess.call(command, cwd=CLI, env=env)
            if status and not exit_code:
                exit_code = status
            if out_dir.exists():
                shutil.move(str(out_dir), str(stage_dir / sub_suite))
        complete = merge_focused_reports(stage_dir, selected, out_dir)
    if not complete and not exit_code:
        exit_code = 1
    return exit_code


def run(args):
    test_tool = resolve_test_tool(args.test_tool)
    if args.suite == "focused":
        if args.partition or args.exact is not None:
            raise ValueError(
                "The consolidated focused suite does not accept --partition or --exact."
            )
        return run_focused(test_tool)
    if args.exact is not None and args.exact not in list_mutants(args.suite):
        raise ValueError("Mutation is outside the selected suite or its description has changed.")
    command = build_run_command(
        args.suite,
        test_tool=test_tool,
        partition=args.partition or "",
        exact=args.exact,
    )
    return subprocess.call(command, cwd=CLI, env=mutation_env())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_subparsers(dest="mode", required=True)
    modes.add_parser("plan")
    execute = modes.add_parser("run")
    execute.add_argument(
        "suite",
        choices=("linux", "linux-lib", "linux-app", "focused", *FOCUSED, "windows"),
    )
    execute.add_argument("--test-tool", choices=("auto", "nextest", "cargo"), default="auto")
    exclusive = execute.add_mutually_exclusive_group()
    exclusive.add_argument("--partition")
    exclusive.add_argument("--exact")
    args = parser.parse_args()
    try:
        return plan() if args.mode == "plan" else run(args)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"Mutation {args.mode} failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
