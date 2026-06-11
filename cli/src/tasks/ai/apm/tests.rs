//! Unit tests for the APM package install task.

use super::autopilot::{WORKFLOW_AUTOPILOT_SCRIPT, WORKFLOW_DESIRED_IDS_SCRIPT};
use super::*;
use crate::exec::{ExecResult, MockExecutor};
use crate::platform::{Os, Platform};
use crate::tasks::test_helpers::{empty_config, make_context, make_linux_context};
use std::sync::Arc;

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

fn write_fragment(dir: &Path, filename: &str, content: &str) {
    std::fs::create_dir_all(dir).expect("create fragment dir");
    std::fs::write(dir.join(filename), content).expect("write manifest fragment");
}

fn write_repo_fragment(root: &Path, filename: &str, content: &str) {
    write_fragment(
        &root.join("symlinks").join("apm").join("config"),
        filename,
        content,
    );
}

fn write_home_fragment(home: &Path, filename: &str, content: &str) {
    write_fragment(&home.join(".apm").join("config"), filename, content);
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

fn make_home_context(home: &Path) -> Context {
    make_linux_context(empty_config(home.to_path_buf())).with_home(home.to_path_buf())
}

fn make_home_context_with_executor(home: &Path, executor: MockExecutor) -> Context {
    make_context(
        empty_config(home.to_path_buf()),
        Platform::new(Os::Linux, false),
        Arc::new(executor),
    )
    .with_home(home.to_path_buf())
}

#[test]
fn should_run_false_when_no_fragments() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = make_home_context(dir.path());
    assert!(!InstallApmPackages.should_run(&ctx));
}

#[test]
fn should_run_true_when_repo_yaml_fragment_exists() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_repo_fragment(dir.path(), "team.yaml", "name: test\n");
    let config = empty_config(dir.path().to_path_buf());
    let ctx = make_linux_context(config);
    assert!(InstallApmPackages.should_run(&ctx));
}

#[test]
fn should_run_true_when_only_overlay_fragment_in_home() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_home_fragment(dir.path(), "work.yml", "name: work\n");
    let ctx = make_home_context(dir.path());
    assert!(InstallApmPackages.should_run(&ctx));
}

#[test]
fn run_skips_when_apm_not_found() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_home_fragment(dir.path(), "base.yml", "name: base\n");

    let ctx = make_home_context(dir.path());
    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    match result {
        TaskResult::Skipped(reason) => assert!(
            reason.contains("apm not found"),
            "expected reason to mention 'apm not found', got {reason:?}"
        ),
        other => panic!("expected TaskResult::Skipped, got {other:?}"),
    }
}

#[test]
fn run_installs_when_manifest_changed() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_home_fragment(
        dir.path(),
        "base.yml",
        "name: base\nversion: 1.0.0\ndependencies:\n  apm:\n    - github/awesome-copilot/plugins/project-planning\n",
    );

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| true);
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, args, env| {
            assert_eq!(args, ["experimental", "enable", "copilot-app"]);
            assert!(env.contains(&("GIT_TERMINAL_PROMPT", "0")));
            Ok(ok_result("[!] copilot-app is already enabled.\n"))
        });
    let install_cwd = dir.path().to_path_buf();
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(move |dir, program, args, env| {
            assert_eq!(dir, install_cwd.as_path());
            assert_eq!(program, "apm");
            assert_eq!(
                args,
                ["install", "-g", "--target", "copilot,vscode,copilot-app"]
            );
            assert!(env.contains(&("GIT_TERMINAL_PROMPT", "0")));
            assert!(env.contains(&("GCM_INTERACTIVE", "Never")));
            assert!(env.contains(&("GCM_GUI_PROMPT", "false")));
            Ok(ok_result("installed\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok after apm install, got {result:?}"
    );
    let manifest = std::fs::read_to_string(dir.path().join(".apm").join("apm.yml"))
        .expect("read merged manifest");
    assert!(manifest.contains("github/awesome-copilot/plugins/project-planning"));
}

/// Shared fragment that forces the changed-manifest install path.
const AUTOPILOT_FIXTURE_FRAGMENT: &str = "name: base\nversion: 1.0.0\ndependencies:\n  apm:\n    - github/awesome-copilot/plugins/project-planning\n";

/// Create `<home>/.copilot/data.db` so the autopilot fixup runs instead of
/// short-circuiting on the missing-database gate.  Returns the db path.
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
            assert_eq!(
                args,
                ["install", "-g", "--target", "copilot,vscode,copilot-app"]
            );
            Ok(ok_result("installed\n"))
        });
}

#[test]
fn run_sets_apm_workflows_to_autopilot_after_install() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_home_fragment(dir.path(), "base.yml", AUTOPILOT_FIXTURE_FRAGMENT);
    let db_path = write_workflow_db(dir.path());
    // The lockfile records exactly the workflows this install deployed; the
    // fixup must scope every query to these ids and nothing else.
    write_workflow_lock(dir.path(), &["apm--a", "apm--b", "apm--c"]);
    let db_str = db_path.to_str().expect("db path utf-8").to_string();

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    // python3 is probed twice: once for the pre-install snapshot and once
    // for the post-install fixup.
    mock.expect_which()
        .with(mockall::predicate::eq("python3"))
        .times(2)
        .returning(|_| true);
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
    write_home_fragment(dir.path(), "base.yml", AUTOPILOT_FIXTURE_FRAGMENT);
    write_workflow_db(dir.path());
    write_workflow_lock(dir.path(), &["apm--a"]);

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_apm_install(&mut mock, &mut seq);
    // Probed twice (pre-install snapshot + post-install fixup); both fall
    // back to `python` and then give up, so neither runs a query.
    mock.expect_which()
        .with(mockall::predicate::eq("python3"))
        .times(2)
        .returning(|_| false);
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
    write_home_fragment(dir.path(), "base.yml", AUTOPILOT_FIXTURE_FRAGMENT);
    write_workflow_db(dir.path());
    write_workflow_lock(dir.path(), &["apm--a"]);

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    mock.expect_which()
        .with(mockall::predicate::eq("python3"))
        .times(2)
        .returning(|_| true);
    // Pre-install snapshot also hits the locked database and degrades to
    // Unavailable, so the post-install fixup stays quiet rather than
    // reporting a spurious change.
    mock.expect_run_unchecked_in()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, _| {
            Ok(ExecResult {
                stdout: String::new(),
                stderr: "database is locked".to_string(),
                success: false,
                code: Some(1),
            })
        });
    expect_apm_install(&mut mock, &mut seq);
    mock.expect_run_unchecked_in()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, _| {
            Ok(ExecResult {
                stdout: String::new(),
                stderr: "database is locked".to_string(),
                success: false,
                code: Some(1),
            })
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);
    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok despite locked db (non-fatal), got {result:?}"
    );
}

#[test]
fn run_warns_when_workflow_db_schema_drifts() {
    // The Copilot App database has drifted from the version-1 workflows schema
    // the embedded scripts target (e.g. the `mode` column was renamed), so
    // sqlite raises `no such column`.  The fixup must surface this loudly while
    // staying non-fatal -- the apm install itself still succeeded.
    let dir = tempfile::tempdir().expect("create temp dir");
    write_home_fragment(dir.path(), "base.yml", AUTOPILOT_FIXTURE_FRAGMENT);
    write_workflow_db(dir.path());
    write_workflow_lock(dir.path(), &["apm--a"]);

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    mock.expect_which()
        .with(mockall::predicate::eq("python3"))
        .times(2)
        .returning(|_| true);
    // Pre-install snapshot hits the drifted schema first.  Since the error is
    // not `no such table`, it degrades quietly to Unavailable.
    mock.expect_run_unchecked_in()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, _| {
            Ok(ExecResult {
                stdout: String::new(),
                stderr: "no such column: mode".to_string(),
                success: false,
                code: Some(1),
            })
        });
    expect_apm_install(&mut mock, &mut seq);
    mock.expect_run_unchecked_in()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, _| {
            Ok(ExecResult {
                stdout: String::new(),
                stderr: "no such column: mode".to_string(),
                success: false,
                code: Some(1),
            })
        });

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
    // lockfile records no `copilot-app-db://workflows/` entries.  The fixup
    // must scope to zero ids and skip entirely -- never probing python or
    // touching the database -- even though `~/.copilot/data.db` exists.
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
fn update_skips_deps_update_when_dependencies_current() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| true);
    let outdated_cwd = dir.path().to_path_buf();
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(move |dir, program, args, env| {
            assert_eq!(dir, outdated_cwd.as_path());
            assert_eq!(program, "apm");
            assert_eq!(args, ["outdated", "-g"]);
            assert!(env.contains(&("GIT_TERMINAL_PROMPT", "0")));
            Ok(ok_result("[*] All dependencies are up-to-date\n"))
        });
    // No `apm deps update` expectation: when nothing is outdated the update
    // task must not advance any locked ref.  The mock panics on extra calls.

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = UpdateApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok when dependencies are already current, got {result:?}"
    );
}

#[test]
fn update_advances_dependencies_when_outdated_reports_stale_lock() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| true);
    let outdated_cwd = dir.path().to_path_buf();
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(move |dir, program, args, _env| {
            assert_eq!(dir, outdated_cwd.as_path());
            assert_eq!(program, "apm");
            assert_eq!(args, ["outdated", "-g"]);
            Ok(ok_result("Outdated dependencies:\n- example/plugin\n"))
        });
    let update_cwd = dir.path().to_path_buf();
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(move |dir, program, args, env| {
            assert_eq!(dir, update_cwd.as_path());
            assert_eq!(program, "apm");
            assert_eq!(
                args,
                [
                    "deps",
                    "update",
                    "-g",
                    "--target",
                    "copilot,vscode,copilot-app"
                ]
            );
            assert!(env.contains(&("GIT_TERMINAL_PROMPT", "0")));
            Ok(ok_result("updated\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = UpdateApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok after apm deps update, got {result:?}"
    );
}

#[test]
fn update_stays_quiet_when_deps_update_reports_no_changes() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| true);
    // Branch/commit refs report an `unknown` status, so `apm outdated`
    // never prints the up-to-date marker and an update is attempted.
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(move |_, _, args, _| {
            assert_eq!(args, ["outdated", "-g"]);
            Ok(ok_result(
                "[i] Some dependencies could not be checked (branch/commit refs)\n",
            ))
        });
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(move |_, _, args, _| {
            assert_eq!(
                args,
                [
                    "deps",
                    "update",
                    "-g",
                    "--target",
                    "copilot,vscode,copilot-app"
                ]
            );
            Ok(ok_result("[*] All packages already at latest refs.\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = UpdateApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok when update made no changes, got {result:?}"
    );
}

#[test]
fn update_re_arms_apm_workflows_after_deps_update() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());
    // Overwrite the plain lock with one that records a dotfiles-managed
    // workflow so the pre-update snapshot and post-update fixup are scoped to
    // it.  This is the regression scenario: `apm deps update` redeploys the
    // workflow secure-by-default (disabled), and the fixup must re-arm it.
    write_workflow_lock(dir.path(), &["apm--a"]);
    let db_path = write_workflow_db(dir.path());
    let db_str = db_path.to_str().expect("db path utf-8").to_string();

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| true);
    // python3 is probed twice: once for the pre-update snapshot and once for
    // the post-update fixup.
    mock.expect_which()
        .with(mockall::predicate::eq("python3"))
        .times(2)
        .returning(|_| true);

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
    // apm deps update advances the lock and redeploys the workflow disabled.
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(move |_, program, args, _| {
            assert_eq!(program, "apm");
            assert_eq!(
                args,
                [
                    "deps",
                    "update",
                    "-g",
                    "--target",
                    "copilot,vscode,copilot-app"
                ]
            );
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
        "expected Ok after re-arming workflows post deps update, got {result:?}"
    );
}

#[test]
fn update_re_arms_apm_workflows_even_when_deps_update_reports_no_changes() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());
    // A dotfiles-managed workflow is recorded in the lock.  Even when
    // `apm deps update` reports no advanced refs it can still redeploy the
    // workflow disabled, so the fixup must run defensively on this path too.
    write_workflow_lock(dir.path(), &["apm--a"]);
    let db_path = write_workflow_db(dir.path());
    let db_str = db_path.to_str().expect("db path utf-8").to_string();

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| true);
    mock.expect_which()
        .with(mockall::predicate::eq("python3"))
        .times(2)
        .returning(|_| true);

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
    // apm deps update reports no advanced refs (Unchanged outcome).
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(move |_, _, args, _| {
            assert_eq!(
                args,
                [
                    "deps",
                    "update",
                    "-g",
                    "--target",
                    "copilot,vscode,copilot-app"
                ]
            );
            Ok(ok_result("[*] All packages already at latest refs.\n"))
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
fn install_task_converges_without_advancing_dependencies() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| true);
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, args, _| {
            assert_eq!(args, ["experimental", "enable", "copilot-app"]);
            Ok(ok_result("[!] copilot-app is already enabled.\n"))
        });
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(move |_, _, args, _| {
            assert_eq!(
                args,
                ["install", "-g", "--target", "copilot,vscode,copilot-app"]
            );
            Ok(ok_result("installed\n"))
        });
    // No `apm outdated` / `apm deps update` expectations are registered: the
    // convergence task never advances locked refs — that is the `update`-only
    // `UpdateApmPackages` task's job.  The mock would panic on any such call.

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok converging without dependency advancement, got {result:?}"
    );
}

#[test]
fn update_skips_advancement_when_install_marker_missing() {
    let dir = tempfile::tempdir().expect("create temp dir");
    // Manifest + lock present but no success marker => the current manifest was
    // never installed successfully, so the update task must NOT contact apm.
    write_current_manifest_and_lock(dir.path());

    let mut mock = MockExecutor::new();
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| true);
    // No `apm outdated` / `apm deps update` expectations: the converged-manifest
    // guard must short-circuit before any lockfile-advancing call.  The mock
    // panics on any unexpected `run_in_with_env`.

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = UpdateApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Skipped(_)),
        "expected Skipped when the install success marker is missing, got {result:?}"
    );
}

#[test]
fn run_installs_when_success_marker_is_missing() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_and_lock(dir.path());

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| true);
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, args, env| {
            assert_eq!(args, ["experimental", "enable", "copilot-app"]);
            assert!(env.contains(&("GIT_TERMINAL_PROMPT", "0")));
            Ok(ok_result("[!] copilot-app is already enabled.\n"))
        });
    let install_cwd = dir.path().to_path_buf();
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(move |dir, program, args, _env| {
            assert_eq!(dir, install_cwd.as_path());
            assert_eq!(program, "apm");
            assert_eq!(
                args,
                ["install", "-g", "--target", "copilot,vscode,copilot-app"]
            );
            Ok(ok_result("installed\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok after installing unmarked manifest, got {result:?}"
    );
    assert!(
        dir.path()
            .join(".apm")
            .join(".dotfiles-manifest.sha256")
            .exists(),
        "successful install should write the manifest success marker"
    );
}

#[test]
fn run_dry_run_reports_planned_apm_work_without_writing() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());

    let ctx = make_home_context(dir.path()).with_dry_run(true);

    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::DryRun),
        "expected DryRun with fragments present, got {result:?}"
    );
    assert!(
        !dir.path().join(".apm").join("apm.yml").exists(),
        "dry-run must not write the generated manifest"
    );
}

#[test]
fn update_dry_run_reports_planned_advancement() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());

    let ctx = make_home_context(dir.path()).with_dry_run(true);

    let result = UpdateApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::DryRun),
        "expected DryRun for the update task with fragments present, got {result:?}"
    );
}

#[test]
fn update_skips_when_apm_not_found() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());

    let mut mock = MockExecutor::new();
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| false);
    // No `run_in_with_env` expectations: a missing apm binary must short-circuit
    // before any command runs.

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = UpdateApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Skipped(_)),
        "expected Skipped when apm is not on PATH, got {result:?}"
    );
}

#[test]
fn run_skips_auth_failures() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| true);
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, args, env| {
            assert_eq!(args, ["experimental", "enable", "copilot-app"]);
            assert!(env.contains(&("GIT_TERMINAL_PROMPT", "0")));
            Ok(ok_result("[!] copilot-app is already enabled.\n"))
        });
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, _, _| {
            Err(anyhow::anyhow!(
                "fatal: Authentication failed; terminal prompts disabled"
            ))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = InstallApmPackages
        .run(&ctx)
        .expect("auth failure should skip");
    match result {
        TaskResult::Skipped(reason) => assert!(
            reason.contains("GitHub authentication"),
            "expected auth skip reason, got {reason:?}"
        ),
        other => panic!("expected TaskResult::Skipped, got {other:?}"),
    }
}

#[test]
fn run_propagates_non_auth_apm_failures() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| true);
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, args, env| {
            assert_eq!(args, ["experimental", "enable", "copilot-app"]);
            assert!(env.contains(&("GIT_TERMINAL_PROMPT", "0")));
            Ok(ok_result("[!] copilot-app is already enabled.\n"))
        });
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, _, _| Err(anyhow::anyhow!("archive extraction failed")));

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let err = InstallApmPackages
        .run(&ctx)
        .expect_err("non-auth failures should propagate");
    assert!(
        format!("{err:#}").contains("archive extraction failed"),
        "expected propagated APM failure, got {err:#}"
    );
}

#[test]
fn run_continues_when_experimental_enable_fails() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| true);
    // A best-effort experimental-enable failure (e.g. an older apm without
    // the `experimental` subcommand) must never abort the install.
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, args, _| {
            assert_eq!(args, ["experimental", "enable", "copilot-app"]);
            Err(anyhow::anyhow!(
                "error: unrecognized subcommand 'experimental'"
            ))
        });
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, args, _| {
            assert_eq!(
                args,
                ["install", "-g", "--target", "copilot,vscode,copilot-app"]
            );
            Ok(ok_result("installed\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = InstallApmPackages
        .run(&ctx)
        .expect("install should continue despite enable failure");
    assert!(matches!(result, TaskResult::Ok));
}

#[test]
fn auth_failure_detection_matches_specific_auth_messages() {
    assert!(looks_like_auth_failure(
        "git failed: HTTP 403 Forbidden while fetching repository"
    ));
    assert!(looks_like_auth_failure(
        "fatal: Authentication failed; terminal prompts disabled"
    ));
}

#[test]
fn auth_failure_detection_ignores_unrelated_credential_text() {
    assert!(!looks_like_auth_failure(
        "credential cache cleanup failed after archive extraction"
    ));
}

/// Realistic `apm install` failure output where every error is the experimental
/// `copilot-app` target refusing to lockfile-encode `.agent.md` agents.
const APM_WORKFLOW_ENCODE_ONLY_OUTPUT: &str = "running apm install\n  \
[x] 2 packages failed:\n    \
+- dotnet/skills/plugins/dotnet-diag -- Failed to integrate primitives from cached \
package: Refusing to lockfile-encode non-APM workflow id: \
'optimizing-dotnet-performance.agent.md'\n    \
+- dotnet/skills/plugins/dotnet-msbuild -- Failed to integrate primitives from cached \
package: Refusing to lockfile-encode non-APM workflow id: 'build-perf.agent.md'\n\
[!] Installed 17 APM dependencies in 0.9s with 2 error(s).\n[!] Install interrupted after 0.9s.\n";

#[test]
fn run_tolerates_copilot_app_workflow_encoding_failures() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| true);
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, args, _| {
            assert_eq!(args, ["experimental", "enable", "copilot-app"]);
            Ok(ok_result("[!] copilot-app is already enabled.\n"))
        });
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, args, _| {
            assert_eq!(
                args,
                ["install", "-g", "--target", "copilot,vscode,copilot-app"]
            );
            Err(anyhow::anyhow!(
                "apm install failed (exit 1): stdout: {APM_WORKFLOW_ENCODE_ONLY_OUTPUT}"
            ))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = InstallApmPackages
        .run(&ctx)
        .expect("benign copilot-app encoding failures should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok when only copilot-app workflow-encoding failed, got {result:?}"
    );
    // A successful (if tolerated) install must persist the manifest marker so
    // subsequent runs stay idempotent.
    assert!(
        dir.path()
            .join(".apm")
            .join(".dotfiles-manifest.sha256")
            .exists(),
        "tolerated install should still write the manifest marker"
    );
}

#[test]
fn run_propagates_when_a_non_encoding_error_is_present() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| true);
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, args, _| {
            assert_eq!(args, ["experimental", "enable", "copilot-app"]);
            Ok(ok_result("[!] copilot-app is already enabled.\n"))
        });
    // One benign encoding failure plus one genuine failure: the task must fail.
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, _, _| {
            Err(anyhow::anyhow!(
                "apm install failed (exit 1): stdout:   [x] 2 packages failed:\n    \
                 +- pkg/a -- Failed to integrate primitives from cached package: \
                 Refusing to lockfile-encode non-APM workflow id: 'a.agent.md'\n    \
                 +- pkg/b -- Network error while downloading package\n\
                 [!] Installed 1 APM dependencies in 0.9s with 2 error(s)."
            ))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    InstallApmPackages
        .run(&ctx)
        .expect_err("a genuine failure mixed with encoding noise must propagate");
}

#[test]
fn workflow_encode_detection_accepts_uniform_encoding_failures() {
    assert_eq!(
        tolerable_workflow_encode_failures(APM_WORKFLOW_ENCODE_ONLY_OUTPUT),
        Some(2)
    );
}

#[test]
fn workflow_encode_detection_survives_line_wrapped_markers() {
    // The marker phrase split across wrapped lines must still be recognised.
    let wrapped = "  [x] 1 packages failed:\n    +- pkg -- Failed to integrate primitives\n\
                   from cached package: Refusing to lockfile-encode\nnon-APM workflow id: \
                   'x.agent.md'\n[!] Installed 5 deps with 1 error(s).";
    assert_eq!(tolerable_workflow_encode_failures(wrapped), Some(1));
}

#[test]
fn workflow_encode_detection_rejects_mixed_failures() {
    let mixed = "  [x] 2 packages failed:\n    +- a -- Failed to integrate primitives from \
                 cached package: Refusing to lockfile-encode non-APM workflow id: 'a.agent.md'\n    \
                 +- b -- Network error\n[!] Installed 1 deps with 2 error(s).";
    assert_eq!(tolerable_workflow_encode_failures(mixed), None);
}

#[test]
fn workflow_encode_detection_fails_closed_without_a_total() {
    // Encoding failure present but no parseable error total: do not tolerate.
    let no_total = "    +- a -- Failed to integrate primitives from cached package: \
                    Refusing to lockfile-encode non-APM workflow id: 'a.agent.md'";
    assert_eq!(tolerable_workflow_encode_failures(no_total), None);
}

#[test]
fn workflow_encode_detection_ignores_unrelated_failures() {
    assert_eq!(
        tolerable_workflow_encode_failures("archive extraction failed with 1 error(s)."),
        None
    );
}
