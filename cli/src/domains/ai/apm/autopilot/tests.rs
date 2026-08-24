use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::engine::Task;
use crate::infra::exec::{ExecResult, MockExecutor};
use crate::test_helpers::{assert_task_changed, assert_task_ok};

use super::super::test_fixture::{
    expect_apm_install, expect_apm_update, expect_copilot_app_enable,
    expect_copilot_app_workflow_install, expect_which_apm, install_task,
    make_home_context_with_executor, update_task, write_copilot_app_db,
    write_current_manifest_lock_and_marker, write_home_fragment,
};
use super::DesiredApmWorkflows;
use super::lockfile::parse_deployed_workflow_ids;
use super::outcome::{
    FixupExecution, FixupFailure, FixupOutcome, decide_fixup_outcome, interpret_fixup_result,
};
use super::scripts::{
    WORKFLOW_AUTOPILOT_SCRIPT, WORKFLOW_DESIRED_IDS_SCRIPT, build_workflow_script_args,
    parse_autopilot_result, parse_desired_ids,
};

fn id_set(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|s| (*s).to_string()).collect()
}

/// Fragment that forces the changed-manifest install path in autopilot tests.
const AUTOPILOT_FIXTURE_FRAGMENT: &str = "name: base\nversion: 1.0.0\ndependencies:\n  apm:\n    - github/awesome-copilot/plugins/project-planning\n";

/// Write a `<home>/.apm/apm.lock.yaml` whose `deployed_files` record `ids` as
/// dotfiles-managed Copilot App workflows, so the autopilot fixup is scoped to
/// exactly those ids.
fn write_workflow_lock(home: &Path, ids: &[&str]) {
    let apm_dir = home.join(".apm");
    std::fs::create_dir_all(&apm_dir).expect("create .apm dir");
    let mut yaml = String::from("dependencies:\n- repo_url: test/pkg\n  deployed_files:\n");
    for id in ids {
        yaml.push_str("  - copilot-app-db://workflows/");
        yaml.push_str(id);
        yaml.push('\n');
    }
    std::fs::write(apm_dir.join("apm.lock.yaml"), yaml).expect("write workflow lock");
}

fn write_autopilot_fixture(home: &Path, ids: &[&str]) -> PathBuf {
    write_home_fragment(home, "base.yml", AUTOPILOT_FIXTURE_FRAGMENT);
    let db_path = write_copilot_app_db(home);
    write_workflow_lock(home, ids);
    db_path
}

fn expect_python3(mock: &mut MockExecutor, times: usize, found: bool) {
    mock.expect_which()
        .with(mockall::predicate::eq("python3"))
        .times(times)
        .returning(move |_| found);
}

type AutopilotResultCase = (&'static str, &'static str, Option<(u64, HashSet<String>)>);

#[test]
fn parse_autopilot_result_cases() {
    let cases: Vec<AutopilotResultCase> = vec![
        (
            "reads header and ids",
            "3 3\napm--a\napm--b\napm--c\n",
            Some((3, id_set(&["apm--a", "apm--b", "apm--c"]))),
        ),
        (
            "allows a header-only, zero-id output",
            "0 0\n",
            Some((0, id_set(&[]))),
        ),
        ("rejects a three-token header", "3 3 oops\napm--a\n", None),
        ("rejects fully empty output", "", None),
        ("rejects blank-line-only output", "\n\n", None),
    ];
    for (case, input, expected) in cases {
        assert_eq!(parse_autopilot_result(input), expected, "case: {case}");
    }
}

#[test]
fn parse_desired_ids_cases() {
    let cases = [
        (
            "filters blank lines and trims whitespace",
            "apm--a\n\n  apm--b  \n\n",
            id_set(&["apm--a", "apm--b"]),
        ),
        ("empty input is an empty set", "", id_set(&[])),
    ];
    for (case, input, expected) in cases {
        assert_eq!(parse_desired_ids(input), expected, "case: {case}");
    }
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
fn parse_deployed_workflow_ids_empty_cases() {
    let cases = [
        (
            "no workflows deployed",
            "dependencies:\n- repo_url: _local/dot-code\n  deployed_files:\n  - \
             .agents/skills/project-hygiene\n",
        ),
        // A bare scalar, a mapping without `dependencies`, and a dependency
        // without `deployed_files` must all yield an empty set rather than
        // panicking or erroring.
        ("bare scalar", "lock\n"),
        ("mapping without dependencies", "name: x\nversion: 1\n"),
        (
            "dependency without deployed_files",
            "dependencies:\n- repo_url: a/b\n",
        ),
        ("not yaml", ": : not yaml : :"),
        // An entry that is exactly the prefix (empty id) must be dropped.
        (
            "bare prefix with no id",
            "dependencies:\n- repo_url: a/b\n  deployed_files:\n  - \
             copilot-app-db://workflows/\n",
        ),
    ];
    for (case, lock) in cases {
        assert!(parse_deployed_workflow_ids(lock).is_empty(), "case: {case}");
    }
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
fn run_sets_apm_workflows_to_autopilot_after_install() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = write_autopilot_fixture(dir.path(), &["apm--a", "apm--b", "apm--c"]);
    let db_str = db_path.to_str().expect("db path utf-8").to_string();

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_python3(&mut mock, 2, true);
    // Pre-install snapshot: scoped to the lockfile ids; none are desired yet,
    // so the diff in the post-install fixup is a genuine "set 3" change.
    let pre_home = dir.path().to_path_buf();
    let pre_db = db_str.clone();
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |spec| {
            assert!(!spec.is_checked());
            assert_eq!(spec.working_dir(), Some(pre_home.as_path()));
            assert_eq!(spec.program(), "python3");
            assert_eq!(
                spec.arguments(),
                [
                    "-c",
                    WORKFLOW_DESIRED_IDS_SCRIPT,
                    pre_db.as_str(),
                    "apm--a",
                    "apm--b",
                    "apm--c"
                ]
            );
            Ok(ExecResult::success(""))
        });
    expect_which_apm(&mut mock, true);
    expect_apm_install(&mut mock, &mut seq, dir.path());
    let post_home = dir.path().to_path_buf();
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |spec| {
            assert!(!spec.is_checked());
            assert_eq!(spec.working_dir(), Some(post_home.as_path()));
            assert_eq!(spec.program(), "python3");
            assert_eq!(
                spec.arguments(),
                [
                    "-c",
                    WORKFLOW_AUTOPILOT_SCRIPT,
                    db_str.as_str(),
                    "apm--a",
                    "apm--b",
                    "apm--c"
                ]
            );
            Ok(ExecResult::success("3 3\napm--a\napm--b\napm--c\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);
    let result = install_task().run(&ctx).expect("run should not error");
    assert_task_changed(&result);
}

#[test]
fn run_warns_when_python_missing_for_autopilot_fixup() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_autopilot_fixture(dir.path(), &["apm--a"]);

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    expect_apm_install(&mut mock, &mut seq, dir.path());
    // Probed twice (pre-install snapshot + post-install fixup); both fall
    // back to `python` and then give up, so neither runs a query.
    expect_python3(&mut mock, 2, false);
    mock.expect_which()
        .with(mockall::predicate::eq("python"))
        .times(2)
        .returning(|_| false);

    let ctx = make_home_context_with_executor(dir.path(), mock);
    let result = install_task().run(&ctx).expect("run should not error");
    assert_task_changed(&result);
}

#[test]
fn run_warns_on_degraded_workflow_db_cases() {
    // A locked database and a schema-drifted database (e.g. the `mode` column
    // was renamed) both degrade the fixup non-fatally: the pre-install
    // snapshot treats the error as `Unavailable` rather than `no such table`,
    // and the apm install itself still succeeds.
    for message in ["database is locked", "no such column: mode"] {
        let dir = tempfile::tempdir().expect("create temp dir");
        write_autopilot_fixture(dir.path(), &["apm--a"]);

        let mut mock = MockExecutor::new();
        let mut seq = mockall::Sequence::new();
        expect_python3(&mut mock, 2, true);
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .returning(move |spec| {
                assert!(!spec.is_checked());
                Ok(ExecResult::failure("", message, Some(1)))
            });
        expect_which_apm(&mut mock, true);
        expect_apm_install(&mut mock, &mut seq, dir.path());
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .returning(move |spec| {
                assert!(!spec.is_checked());
                Ok(ExecResult::failure("", message, Some(1)))
            });

        let ctx = make_home_context_with_executor(dir.path(), mock);
        let result = install_task().run(&ctx).expect("run should not error");
        assert_task_changed(&result);
    }
}

#[test]
fn run_skips_autopilot_fixup_when_lock_lists_no_workflows() {
    // The common case: the deployed deps ship only agents/skills, so the
    // lockfile records no `copilot-app-db://workflows/` entries. The fixup must
    // scope to zero ids and skip entirely -- never probing python or touching
    // the database -- even though `~/.copilot/data.db` exists.
    let dir = tempfile::tempdir().expect("create temp dir");
    write_home_fragment(dir.path(), "base.yml", AUTOPILOT_FIXTURE_FRAGMENT);
    write_copilot_app_db(dir.path());
    std::fs::write(
        dir.path().join(".apm").join("apm.lock.yaml"),
        "dependencies:\n- repo_url: test/pkg\n  deployed_files:\n  - .agents/skills/foo\n",
    )
    .expect("write lock without workflows");

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    // Only the apm install runs; no python probe is queued, so the mock would
    // panic on any unexpected `which("python3")`/`execute` call.
    expect_which_apm(&mut mock, true);
    expect_apm_install(&mut mock, &mut seq, dir.path());

    let ctx = make_home_context_with_executor(dir.path(), mock);
    let result = install_task().run(&ctx).expect("run should not error");
    assert_task_changed(&result);
}

#[test]
fn current_install_delegates_to_apm_and_repairs_autopilot_drift() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());
    write_workflow_lock(dir.path(), &["apm--a"]);
    let db_path = write_copilot_app_db(dir.path());
    let db_str = db_path.to_str().expect("db path utf-8").to_string();

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    expect_python3(&mut mock, 2, true);

    let drift_db = db_str.clone();
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |spec| {
            assert!(!spec.is_checked());
            assert_eq!(
                spec.arguments(),
                [
                    "-c",
                    WORKFLOW_DESIRED_IDS_SCRIPT,
                    drift_db.as_str(),
                    "apm--a"
                ]
            );
            Ok(ExecResult::success(""))
        });
    expect_apm_install(&mut mock, &mut seq, dir.path());
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |spec| {
            assert!(!spec.is_checked());
            assert_eq!(
                spec.arguments(),
                ["-c", WORKFLOW_AUTOPILOT_SCRIPT, db_str.as_str(), "apm--a"]
            );
            Ok(ExecResult::success("1 1\napm--a\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);
    let result = install_task().run(&ctx).expect("run should not error");

    assert_task_changed(&result);
}

#[test]
fn current_install_previews_autopilot_drift_without_repairing_it() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());
    write_workflow_lock(dir.path(), &["apm--a"]);
    write_copilot_app_db(dir.path());

    let ctx = make_home_context_with_executor(dir.path(), MockExecutor::new()).with_dry_run(true);
    let result = install_task().run(&ctx).expect("run should not error");

    assert_task_changed(&result);
}

#[test]
fn update_re_arms_apm_workflows_cases() {
    // Regardless of whether `apm update` advances the lock ("updated\n") or
    // reports no changes (cached), it can redeploy a workflow disabled, so
    // the post-update fixup must run defensively on both paths and re-arm it.
    let cases = [
        (
            "apm update advances the lock: workflow not desired pre-update",
            "",
            "updated\n",
            true,
        ),
        (
            "apm update reports no changes: workflow already desired pre-update",
            "apm--a\n",
            "  [+] github.com/example/plugin (cached)\n",
            false,
        ),
    ];
    for (_case, pre_stdout, update_stdout, expected_changed) in cases {
        let dir = tempfile::tempdir().expect("create temp dir");
        write_current_manifest_lock_and_marker(dir.path());
        // Overwrite the plain lock with one that records a dotfiles-managed
        // workflow so the pre-update snapshot and post-update fixup are
        // scoped to it.
        write_workflow_lock(dir.path(), &["apm--a"]);
        let db_path = write_copilot_app_db(dir.path());
        let db_str = db_path.to_str().expect("db path utf-8").to_string();

        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        expect_which_apm(&mut mock, true);
        expect_python3(&mut mock, 2, true);
        let pre_db = db_str.clone();
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .returning(move |spec| {
                assert!(!spec.is_checked());
                assert_eq!(spec.program(), "python3");
                assert_eq!(
                    spec.arguments(),
                    ["-c", WORKFLOW_DESIRED_IDS_SCRIPT, pre_db.as_str(), "apm--a"]
                );
                Ok(ExecResult::success(pre_stdout))
            });
        expect_apm_update(&mut mock, &mut seq, update_stdout);
        expect_copilot_app_enable(&mut mock, &mut seq);
        expect_copilot_app_workflow_install(&mut mock, &mut seq);
        // Post-update fixup re-arms the workflow to autopilot + enabled.
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .returning(move |spec| {
                assert!(!spec.is_checked());
                assert_eq!(spec.program(), "python3");
                assert_eq!(
                    spec.arguments(),
                    ["-c", WORKFLOW_AUTOPILOT_SCRIPT, db_str.as_str(), "apm--a"]
                );
                Ok(ExecResult::success("1 1\napm--a\n"))
            });

        let ctx = make_home_context_with_executor(dir.path(), mock);

        let result = update_task().run(&ctx).expect("run should not error");
        if expected_changed {
            assert_task_changed(&result);
        } else {
            assert_task_ok(&result);
        }
    }
}

#[test]
fn decide_fixup_outcome_cases() {
    let cases: Vec<(&str, &str, DesiredApmWorkflows, FixupOutcome)> = vec![
        (
            "quiet in steady state: the pre-install workflows are already \
             desired, so the install-reset set matches with no net change",
            "3 3\napm--a\napm--b\napm--c\n",
            DesiredApmWorkflows::Known(id_set(&["apm--a", "apm--b", "apm--c"])),
            FixupOutcome::Quiet,
        ),
        (
            "sets all workflows on first install",
            "3 3\napm--a\napm--b\napm--c\n",
            DesiredApmWorkflows::FirstInstall,
            FixupOutcome::Set(3),
        ),
        (
            "sets one when a workflow was newly added",
            "3 3\napm--a\napm--b\napm--c\n",
            DesiredApmWorkflows::Known(id_set(&["apm--a", "apm--b"])),
            FixupOutcome::Set(1),
        ),
        (
            "sets one when the user disabled apm--b pre-install and the fixup \
             re-enables it",
            "3 3\napm--a\napm--b\napm--c\n",
            DesiredApmWorkflows::Known(id_set(&["apm--a", "apm--c"])),
            FixupOutcome::Set(1),
        ),
        (
            "stays quiet when a pre-install workflow is gone post-install, since \
             the post set is a subset of pre and the forward diff is zero",
            "2 2\napm--a\napm--b\n",
            DesiredApmWorkflows::Known(id_set(&["apm--a", "apm--b", "apm--c"])),
            FixupOutcome::Quiet,
        ),
        (
            "reports no workflows when the header lists zero",
            "0 0\n",
            DesiredApmWorkflows::FirstInstall,
            FixupOutcome::NoWorkflows,
        ),
        (
            "stays quiet without a trustworthy pre-install snapshot, rather than \
             emit a spurious \"set N\" line",
            "3 3\napm--a\napm--b\napm--c\n",
            DesiredApmWorkflows::Unavailable,
            FixupOutcome::Quiet,
        ),
        (
            "reports unparsed on a non-numeric header",
            "not-a-number\n",
            DesiredApmWorkflows::FirstInstall,
            FixupOutcome::Unparsed,
        ),
        (
            "reports unparsed on a three-token header",
            "3 3 extra\napm--a\n",
            DesiredApmWorkflows::FirstInstall,
            FixupOutcome::Unparsed,
        ),
    ];
    for (case, stdout, pre, expected) in cases {
        assert_eq!(decide_fixup_outcome(stdout, &pre), expected, "case: {case}");
    }
}

#[test]
fn interpret_fixup_result_classifies_script_failures() {
    let pre = DesiredApmWorkflows::Unavailable;
    assert_eq!(
        interpret_fixup_result(ExecResult::failure("", "database is locked", Some(1)), &pre,),
        FixupExecution::Failed(FixupFailure::DatabaseLocked)
    );
    assert_eq!(
        interpret_fixup_result(
            ExecResult::failure("", "no such table: workflows", Some(1)),
            &pre,
        ),
        FixupExecution::Failed(FixupFailure::WorkflowsTableMissing)
    );
    assert_eq!(
        interpret_fixup_result(
            ExecResult::failure("", "no such column: mode", Some(1)),
            &pre,
        ),
        FixupExecution::Failed(FixupFailure::SchemaDrift(
            "no such column: mode".to_string()
        ))
    );
    assert_eq!(
        interpret_fixup_result(ExecResult::failure("", "permission denied", Some(1)), &pre,),
        FixupExecution::Failed(FixupFailure::Other("permission denied".to_string()))
    );
}
