#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::exec::{ExecResult, MockExecutor};
use crate::platform::{Os, Platform};
use crate::tasks::test_helpers::{empty_config, make_context};
use crate::tasks::{Context, Task, TaskResult};

use super::super::fragments::{discover_fragment_files, merge_fragments};
use super::super::manifest::{manifest_fingerprint, write_manifest_marker};
use super::super::{InstallApmPackages, UpdateApmPackages};
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

fn ok_result(stdout: &str) -> ExecResult {
    ExecResult {
        stdout: stdout.to_string(),
        stderr: String::new(),
        success: true,
        code: Some(0),
    }
}

const DEFAULT_FRAGMENT: &str =
    "name: base\nversion: 1.0.0\ndependencies:\n  apm:\n    - example/plugin\n";

/// Shared fragment that forces the changed-manifest install path.
const AUTOPILOT_FIXTURE_FRAGMENT: &str = "name: base\nversion: 1.0.0\ndependencies:\n  apm:\n    - github/awesome-copilot/plugins/project-planning\n";
const TARGET_ALL: &str = "copilot,codex,copilot-app";

fn write_home_fragment(home: &Path, filename: &str, content: &str) {
    let fragment_dir = home.join(".apm").join("config");
    std::fs::create_dir_all(&fragment_dir).expect("create fragment dir");
    std::fs::write(fragment_dir.join(filename), content).expect("write manifest fragment");
}

fn write_default_home_fragment(home: &Path) {
    write_home_fragment(home, "base.yml", DEFAULT_FRAGMENT);
}

fn write_current_manifest_and_lock(home: &Path) {
    write_default_home_fragment(home);
    let fragments = discover_fragment_files(home).expect("discover fragments");
    let merged = merge_fragments(&fragments).expect("merge fragments");
    std::fs::write(home.join(".apm").join("apm.yml"), merged).expect("write manifest");
    std::fs::write(home.join(".apm").join("apm.lock.yaml"), "lock\n").expect("write lock");
}

fn write_current_manifest_lock_and_marker(home: &Path) {
    write_current_manifest_and_lock(home);
    let manifest =
        std::fs::read_to_string(home.join(".apm").join("apm.yml")).expect("read manifest");
    write_manifest_marker(
        &home.join(".apm").join(".dotfiles-manifest.sha256"),
        &manifest_fingerprint(&manifest),
    )
    .expect("write marker");
}

fn make_home_context_with_executor(home: &Path, executor: MockExecutor) -> Context {
    write_workflow_db(home);
    make_context(
        empty_config(home.to_path_buf()),
        Platform::new(Os::Linux, false),
        Arc::new(executor),
    )
    .with_home(home.to_path_buf())
}

/// Create `<home>/.copilot/data.db` so the autopilot fixup runs instead of
/// short-circuiting on the missing-database gate. Returns the db path.
fn write_workflow_db(home: &Path) -> PathBuf {
    let copilot_dir = home.join(".copilot");
    std::fs::create_dir_all(&copilot_dir).expect("create .copilot dir");
    let db_path = copilot_dir.join("data.db");
    std::fs::write(&db_path, b"db").expect("write data.db");
    db_path
}

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
    let db_path = write_workflow_db(home);
    write_workflow_lock(home, ids);
    db_path
}

fn expect_python3(mock: &mut MockExecutor, times: usize, found: bool) {
    mock.expect_which()
        .with(mockall::predicate::eq("python3"))
        .times(times)
        .returning(move |_| found);
}

fn expect_apm_available(mock: &mut MockExecutor) {
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| true);
}

fn exec_error(stderr: &str) -> ExecResult {
    ExecResult {
        stdout: String::new(),
        stderr: stderr.to_string(),
        success: false,
        code: Some(1),
    }
}

/// Queue the apm `which` + experimental-enable + install expectations shared
/// by every autopilot-fixup test (the changed-manifest path).
fn expect_apm_install(mock: &mut MockExecutor, seq: &mut mockall::Sequence) {
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| true);
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(seq)
        .returning(|_, _, args, _| {
            assert_eq!(args, ["experimental", "enable", "copilot-app"]);
            Ok(ok_result("[!] copilot-app is already enabled.\n"))
        });
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(seq)
        .returning(|_, program, args, _| {
            assert_eq!(program, "apm");
            assert_eq!(args, ["install", "-g", "--target", TARGET_ALL]);
            Ok(ok_result("installed\n"))
        });
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
    mock.expect_run_unchecked_in()
        .once()
        .in_sequence(&mut seq)
        .returning(move |run_dir, program, args| {
            assert_eq!(run_dir, pre_home.as_path());
            assert_eq!(program, "python3");
            assert_eq!(
                args,
                [
                    "-c",
                    WORKFLOW_DESIRED_IDS_SCRIPT,
                    pre_db.as_str(),
                    "apm--a",
                    "apm--b",
                    "apm--c"
                ]
            );
            Ok(ok_result(""))
        });
    expect_apm_install(&mut mock, &mut seq);
    let post_home = dir.path().to_path_buf();
    mock.expect_run_unchecked_in()
        .once()
        .in_sequence(&mut seq)
        .returning(move |run_dir, program, args| {
            assert_eq!(run_dir, post_home.as_path());
            assert_eq!(program, "python3");
            assert_eq!(
                args,
                [
                    "-c",
                    WORKFLOW_AUTOPILOT_SCRIPT,
                    db_str.as_str(),
                    "apm--a",
                    "apm--b",
                    "apm--c"
                ]
            );
            Ok(ok_result("3 3\napm--a\napm--b\napm--c\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);
    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok after autopilot fixup, got {result:?}"
    );
}

#[test]
fn run_warns_when_python_missing_for_autopilot_fixup() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_autopilot_fixture(dir.path(), &["apm--a"]);

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_apm_install(&mut mock, &mut seq);
    // Probed twice (pre-install snapshot + post-install fixup); both fall
    // back to `python` and then give up, so neither runs a query.
    expect_python3(&mut mock, 2, false);
    mock.expect_which()
        .with(mockall::predicate::eq("python"))
        .times(2)
        .returning(|_| false);

    let ctx = make_home_context_with_executor(dir.path(), mock);
    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok when python is missing (non-fatal), got {result:?}"
    );
}

#[test]
fn run_warns_when_workflow_db_is_locked() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_autopilot_fixture(dir.path(), &["apm--a"]);

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_python3(&mut mock, 2, true);
    // Pre-install snapshot also hits the locked database and degrades to
    // Unavailable, so the post-install fixup stays quiet rather than reporting
    // a spurious change.
    mock.expect_run_unchecked_in()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, _| Ok(exec_error("database is locked")));
    expect_apm_install(&mut mock, &mut seq);
    mock.expect_run_unchecked_in()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, _| Ok(exec_error("database is locked")));

    let ctx = make_home_context_with_executor(dir.path(), mock);
    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok despite locked db (non-fatal), got {result:?}"
    );
}

#[test]
fn run_warns_when_workflow_db_schema_drifts() {
    // The Copilot App database has drifted from the version-2 workflows schema
    // the embedded scripts target (e.g. the `mode` column was renamed), so
    // sqlite raises `no such column`. The fixup must surface this loudly while
    // staying non-fatal -- the apm install itself still succeeded.
    let dir = tempfile::tempdir().expect("create temp dir");
    write_autopilot_fixture(dir.path(), &["apm--a"]);

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_python3(&mut mock, 2, true);
    // Pre-install snapshot hits the drifted schema first. Since the error is
    // not `no such table`, it degrades quietly to Unavailable.
    mock.expect_run_unchecked_in()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, _| Ok(exec_error("no such column: mode")));
    expect_apm_install(&mut mock, &mut seq);
    mock.expect_run_unchecked_in()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, _| Ok(exec_error("no such column: mode")));

    let ctx = make_home_context_with_executor(dir.path(), mock);
    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok despite schema drift (non-fatal), got {result:?}"
    );
}

#[test]
fn run_skips_autopilot_fixup_when_lock_lists_no_workflows() {
    // The common case: the deployed deps ship only agents/skills, so the
    // lockfile records no `copilot-app-db://workflows/` entries. The fixup must
    // scope to zero ids and skip entirely -- never probing python or touching
    // the database -- even though `~/.copilot/data.db` exists.
    let dir = tempfile::tempdir().expect("create temp dir");
    write_home_fragment(dir.path(), "base.yml", AUTOPILOT_FIXTURE_FRAGMENT);
    write_workflow_db(dir.path());
    std::fs::write(
        dir.path().join(".apm").join("apm.lock.yaml"),
        "dependencies:\n- repo_url: test/pkg\n  deployed_files:\n  - .agents/skills/foo\n",
    )
    .expect("write lock without workflows");

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    // Only the apm install runs; no python probe is queued, so the mock would
    // panic on any unexpected `which("python3")`/`run_unchecked_in` call.
    expect_apm_install(&mut mock, &mut seq);

    let ctx = make_home_context_with_executor(dir.path(), mock);
    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok with the fixup skipped, got {result:?}"
    );
}

#[test]
fn update_re_arms_apm_workflows_after_apm_update() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());
    // Overwrite the plain lock with one that records a dotfiles-managed
    // workflow so the pre-update snapshot and post-update fixup are scoped to
    // it. This is the regression scenario: `apm update` redeploys the workflow
    // secure-by-default (disabled), and the fixup must re-arm it.
    write_workflow_lock(dir.path(), &["apm--a"]);
    let db_path = write_workflow_db(dir.path());
    let db_str = db_path.to_str().expect("db path utf-8").to_string();

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    expect_apm_available(&mut mock);
    expect_python3(&mut mock, 2, true);

    // apm outdated reports a stale lock, so an update is attempted.
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(move |_, program, args, _| {
            assert_eq!(program, "apm");
            assert_eq!(args, ["outdated", "-g"]);
            Ok(ok_result("Outdated dependencies:\n- example/plugin\n"))
        });
    // Pre-update snapshot: the workflow is not desired yet, so the later diff
    // is a genuine "set 1" change.
    let pre_db = db_str.clone();
    mock.expect_run_unchecked_in()
        .once()
        .in_sequence(&mut seq)
        .returning(move |_, program, args| {
            assert_eq!(program, "python3");
            assert_eq!(
                args,
                ["-c", WORKFLOW_DESIRED_IDS_SCRIPT, pre_db.as_str(), "apm--a"]
            );
            Ok(ok_result(""))
        });
    // apm update advances the lock and redeploys the workflow disabled.
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(move |_, program, args, _| {
            assert_eq!(program, "apm");
            assert_eq!(args, ["update", "-g", "--yes", "--target", TARGET_ALL]);
            Ok(ok_result("updated\n"))
        });
    // Post-update fixup re-arms the workflow to autopilot + enabled; the diff
    // against the empty pre-snapshot reports one newly desired workflow.
    mock.expect_run_unchecked_in()
        .once()
        .in_sequence(&mut seq)
        .returning(move |_, program, args| {
            assert_eq!(program, "python3");
            assert_eq!(
                args,
                ["-c", WORKFLOW_AUTOPILOT_SCRIPT, db_str.as_str(), "apm--a"]
            );
            Ok(ok_result("1 1\napm--a\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = UpdateApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok after re-arming workflows post apm update, got {result:?}"
    );
}

#[test]
fn update_re_arms_apm_workflows_even_when_apm_update_reports_no_changes() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());
    // A dotfiles-managed workflow is recorded in the lock. Even when
    // `apm update` reports no advanced refs it can still redeploy the workflow
    // disabled, so the fixup must run defensively on this path too.
    write_workflow_lock(dir.path(), &["apm--a"]);
    let db_path = write_workflow_db(dir.path());
    let db_str = db_path.to_str().expect("db path utf-8").to_string();

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    expect_apm_available(&mut mock);
    expect_python3(&mut mock, 2, true);

    // Branch/commit refs report `unknown`, so the up-to-date marker is absent
    // and an update is attempted.
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(move |_, _, args, _| {
            assert_eq!(args, ["outdated", "-g"]);
            Ok(ok_result(
                "[i] Some dependencies could not be checked (branch/commit refs)\n",
            ))
        });
    // Pre-update snapshot: the workflow is already desired (steady state).
    let pre_db = db_str.clone();
    mock.expect_run_unchecked_in()
        .once()
        .in_sequence(&mut seq)
        .returning(move |_, program, args| {
            assert_eq!(program, "python3");
            assert_eq!(
                args,
                ["-c", WORKFLOW_DESIRED_IDS_SCRIPT, pre_db.as_str(), "apm--a"]
            );
            Ok(ok_result("apm--a\n"))
        });
    // apm update leaves the lockfile untouched (Unchanged outcome).
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(move |_, _, args, _| {
            assert_eq!(args, ["update", "-g", "--yes", "--target", TARGET_ALL]);
            Ok(ok_result("  [+] github.com/example/plugin (cached)\n"))
        });
    // The fixup still runs; with the workflow already desired the delta is
    // net-zero, so it stays quiet but must not be skipped.
    mock.expect_run_unchecked_in()
        .once()
        .in_sequence(&mut seq)
        .returning(move |_, program, args| {
            assert_eq!(program, "python3");
            assert_eq!(
                args,
                ["-c", WORKFLOW_AUTOPILOT_SCRIPT, db_str.as_str(), "apm--a"]
            );
            Ok(ok_result("1 1\napm--a\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = UpdateApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok after re-arming workflows on the no-change path, got {result:?}"
    );
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
