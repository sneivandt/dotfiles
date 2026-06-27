#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]

use std::collections::{BTreeSet, HashSet};

use super::DesiredApmWorkflows;
use super::lockfile::parse_deployed_workflow_ids;
use super::outcome::{FixupOutcome, decide_fixup_outcome};
use super::scripts::{
    WORKFLOW_AUTOPILOT_SCRIPT, WORKFLOW_DESIRED_IDS_SCRIPT, build_workflow_script_args,
    parse_autopilot_result, parse_desired_ids,
};

fn id_set(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn parse_autopilot_result_reads_header_and_ids() {
    let (matched, ids) =
        parse_autopilot_result("3 3\napm--a\napm--b\napm--c\n").expect("valid output parses");
    assert_eq!(matched, 3);
    assert_eq!(ids, id_set(&["apm--a", "apm--b", "apm--c"]));
}

#[test]
fn parse_autopilot_result_allows_zero_ids() {
    let (matched, ids) = parse_autopilot_result("0 0\n").expect("header-only output parses");
    assert_eq!(matched, 0);
    assert!(ids.is_empty());
}

#[test]
fn parse_autopilot_result_rejects_three_token_header() {
    assert!(parse_autopilot_result("3 3 oops\napm--a\n").is_none());
}

#[test]
fn parse_autopilot_result_rejects_empty_output() {
    assert!(parse_autopilot_result("").is_none());
    assert!(parse_autopilot_result("\n\n").is_none());
}

#[test]
fn parse_desired_ids_filters_blank_lines() {
    let ids = parse_desired_ids("apm--a\n\n  apm--b  \n\n");
    assert_eq!(ids, id_set(&["apm--a", "apm--b"]));
}

#[test]
fn parse_desired_ids_empty_is_empty_set() {
    assert!(parse_desired_ids("").is_empty());
}

#[test]
fn parse_deployed_workflow_ids_extracts_only_workflow_uris() {
    let lock = "\
lockfile_version: '1'
dependencies:
- repo_url: _local/dot-code
  deployed_files:
  - .agents/skills/project-hygiene
- repo_url: github/awesome-copilot
  deployed_files:
  - copilot-app-db://workflows/apm--awesome-copilot--planning--triage
  - .agents/skills/foo
  - copilot-app-db://workflows/apm--awesome-copilot--planning--report
- repo_url: dotnet/skills
  deployed_files:
  - copilot-app-db://workflows/apm--dotnet--diag--collect
";
    let ids = parse_deployed_workflow_ids(lock);
    let expected: BTreeSet<String> = [
        "apm--awesome-copilot--planning--triage",
        "apm--awesome-copilot--planning--report",
        "apm--dotnet--diag--collect",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    assert_eq!(ids, expected);
}

#[test]
fn parse_deployed_workflow_ids_empty_when_no_workflows() {
    let lock = "\
dependencies:
- repo_url: _local/dot-code
  deployed_files:
  - .agents/skills/project-hygiene
";
    assert!(parse_deployed_workflow_ids(lock).is_empty());
}

#[test]
fn parse_deployed_workflow_ids_empty_on_malformed_or_unrelated_yaml() {
    // A bare scalar, a mapping without `dependencies`, and a dependency
    // without `deployed_files` must all yield an empty set rather than
    // panicking or erroring.
    assert!(parse_deployed_workflow_ids("lock\n").is_empty());
    assert!(parse_deployed_workflow_ids("name: x\nversion: 1\n").is_empty());
    assert!(parse_deployed_workflow_ids("dependencies:\n- repo_url: a/b\n").is_empty());
    assert!(parse_deployed_workflow_ids(": : not yaml : :").is_empty());
}

#[test]
fn parse_deployed_workflow_ids_ignores_bare_prefix() {
    // An entry that is exactly the prefix (empty id) must be dropped.
    let lock = "\
dependencies:
- repo_url: a/b
  deployed_files:
  - copilot-app-db://workflows/
";
    assert!(parse_deployed_workflow_ids(lock).is_empty());
}

#[test]
fn build_workflow_script_args_appends_ids_in_order() {
    let ids = vec!["apm--a".to_string(), "apm--b".to_string()];
    let args = build_workflow_script_args(WORKFLOW_AUTOPILOT_SCRIPT, "/db", &ids);
    assert_eq!(
        args,
        ["-c", WORKFLOW_AUTOPILOT_SCRIPT, "/db", "apm--a", "apm--b"]
    );
}

fn python_for_script_tests() -> Option<&'static str> {
    ["python3", "python"].into_iter().find(|candidate| {
        std::process::Command::new(candidate)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}

fn run_python_script(python: &str, script: &str, args: &[&str]) -> std::process::Output {
    std::process::Command::new(python)
        .arg("-c")
        .arg(script)
        .args(args)
        .output()
        .expect("run python script")
}

#[test]
fn workflow_autopilot_script_deduplicates_managed_rows() {
    let Some(python) = python_for_script_tests() else {
        return;
    };
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("data.db");
    let db = db_path.to_str().expect("db path utf-8");
    let setup = run_python_script(
        python,
        r#"
import sqlite3, sys
con = sqlite3.connect(sys.argv[1])
con.execute("CREATE TABLE workflows (id TEXT, name TEXT, prompt TEXT, mode TEXT, enabled INTEGER, interval TEXT, schedule_hour INTEGER, schedule_minute INTEGER, schedule_day INTEGER, next_run_at TEXT)")
con.executemany(
    "INSERT INTO workflows VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    [
        ("apm--unknown--a", "PR Triage", "prompt-a", "autopilot", 1, "hourly", 9, 0, 1, "2099-01-01T00:00:00.000Z"),
        ("apm--a", "PR Triage", "prompt-a", "autopilot", 1, "hourly", 9, 0, 1, "2099-01-01T00:00:00.000Z"),
        ("apm--a", "PR Triage", "prompt-a", "interactive", 0, "hourly", 9, 0, 1, None),
        ("apm--b", "PR Review", "prompt-b", "interactive", 0, "daily", 9, 0, 1, None),
        ("foreign--workflow", "Foreign", "prompt-foreign", "interactive", 0, "hourly", 9, 0, 1, None),
        ("foreign--workflow", "Foreign", "prompt-foreign", "interactive", 0, "hourly", 9, 0, 1, None),
    ],
)
con.commit()
"#,
        &[db],
    );
    assert!(
        setup.status.success(),
        "setup failed: {}",
        String::from_utf8_lossy(&setup.stderr)
    );

    let fixup = run_python_script(python, WORKFLOW_AUTOPILOT_SCRIPT, &[db, "apm--a", "apm--b"]);
    assert!(
        fixup.status.success(),
        "fixup failed: {}",
        String::from_utf8_lossy(&fixup.stderr)
    );
    assert_eq!(
        String::from_utf8(fixup.stdout)
            .expect("stdout utf-8")
            .replace("\r\n", "\n"),
        "3 2\napm--a\napm--b\n"
    );

    let query = run_python_script(
        python,
        r#"
import sqlite3, sys
con = sqlite3.connect(sys.argv[1])
for row in con.execute("SELECT id, COUNT(*), MIN(mode), MIN(enabled) FROM workflows GROUP BY id ORDER BY id"):
    print("|".join(map(str, row)))
"#,
        &[db],
    );
    assert!(
        query.status.success(),
        "query failed: {}",
        String::from_utf8_lossy(&query.stderr)
    );
    assert_eq!(
        String::from_utf8(query.stdout)
            .expect("stdout utf-8")
            .replace("\r\n", "\n"),
        "apm--a|1|autopilot|1\napm--b|1|autopilot|1\nforeign--workflow|2|interactive|0\n"
    );
}

/// Regression guard: the embedded Python scripts must keep the `print`
/// body indented under its `for` loop. Rust string `\`-continuations strip
/// the leading whitespace of the next source line, which previously
/// flattened the indent and produced an `IndentationError` at real install
/// time (dry-run never executes these scripts, so only a live install hit
/// it). Assert the runtime bytes carry a four-space indented `print`.
#[test]
fn workflow_scripts_keep_python_indentation() {
    for script in [WORKFLOW_DESIRED_IDS_SCRIPT, WORKFLOW_AUTOPILOT_SCRIPT] {
        assert!(
            script.contains("):\n    print(row[0])\n"),
            "script must indent the for-loop body by four spaces:\n{script}"
        );
        assert!(
            !script.contains("):\nprint("),
            "script must not flatten the for-loop body indentation:\n{script}"
        );
    }
}

#[test]
fn decide_fixup_outcome_quiet_in_steady_state() {
    // Pre-install the three workflows are already desired; the install resets
    // them and the fixup restores the same three -- no net change.
    let pre = DesiredApmWorkflows::Known(id_set(&["apm--a", "apm--b", "apm--c"]));
    let outcome = decide_fixup_outcome("3 3\napm--a\napm--b\napm--c\n", &pre);
    assert_eq!(outcome, FixupOutcome::Quiet);
}

#[test]
fn decide_fixup_outcome_set_all_on_first_install() {
    let pre = DesiredApmWorkflows::FirstInstall;
    let outcome = decide_fixup_outcome("3 3\napm--a\napm--b\napm--c\n", &pre);
    assert_eq!(outcome, FixupOutcome::Set(3));
}

#[test]
fn decide_fixup_outcome_set_one_when_workflow_added() {
    let pre = DesiredApmWorkflows::Known(id_set(&["apm--a", "apm--b"]));
    let outcome = decide_fixup_outcome("3 3\napm--a\napm--b\napm--c\n", &pre);
    assert_eq!(outcome, FixupOutcome::Set(1));
}

#[test]
fn decide_fixup_outcome_set_one_when_user_disabled_then_reenabled() {
    // apm--b was disabled by the user pre-install; the fixup re-enables it.
    let pre = DesiredApmWorkflows::Known(id_set(&["apm--a", "apm--c"]));
    let outcome = decide_fixup_outcome("3 3\napm--a\napm--b\napm--c\n", &pre);
    assert_eq!(outcome, FixupOutcome::Set(1));
}

#[test]
fn decide_fixup_outcome_quiet_when_workflow_removed() {
    // A workflow desired pre-install is gone post-install: the post set is a
    // subset of pre, so the forward diff is zero and nothing was set.
    let pre = DesiredApmWorkflows::Known(id_set(&["apm--a", "apm--b", "apm--c"]));
    let outcome = decide_fixup_outcome("2 2\napm--a\napm--b\n", &pre);
    assert_eq!(outcome, FixupOutcome::Quiet);
}

#[test]
fn decide_fixup_outcome_reports_no_workflows_when_absent() {
    let pre = DesiredApmWorkflows::FirstInstall;
    let outcome = decide_fixup_outcome("0 0\n", &pre);
    assert_eq!(outcome, FixupOutcome::NoWorkflows);
}

#[test]
fn decide_fixup_outcome_quiet_when_snapshot_unavailable() {
    // Without a trustworthy pre-install snapshot we cannot prove a change, so
    // stay quiet rather than emit a spurious "set N" line.
    let pre = DesiredApmWorkflows::Unavailable;
    let outcome = decide_fixup_outcome("3 3\napm--a\napm--b\napm--c\n", &pre);
    assert_eq!(outcome, FixupOutcome::Quiet);
}

#[test]
fn decide_fixup_outcome_unparsed_on_malformed_output() {
    let pre = DesiredApmWorkflows::FirstInstall;
    assert_eq!(
        decide_fixup_outcome("not-a-number\n", &pre),
        FixupOutcome::Unparsed
    );
    assert_eq!(
        decide_fixup_outcome("3 3 extra\napm--a\n", &pre),
        FixupOutcome::Unparsed
    );
}
