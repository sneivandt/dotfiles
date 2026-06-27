//! Unit tests for the APM package install task.

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
    write_copilot_app_db(home);
    make_home_context_without_copilot_app_with_executor(home, executor)
}

fn make_home_context_without_copilot_app_with_executor(
    home: &Path,
    executor: MockExecutor,
) -> Context {
    make_home_context_for_platform_with_executor(home, Platform::new(Os::Linux, false), executor)
}

fn make_home_context_for_platform_with_executor(
    home: &Path,
    platform: Platform,
    executor: MockExecutor,
) -> Context {
    make_context(
        empty_config(home.to_path_buf()),
        platform,
        Arc::new(executor),
    )
    .with_home(home.to_path_buf())
}

fn make_home_context_for_platform(home: &Path, platform: Platform) -> Context {
    make_home_context_for_platform_with_executor(home, platform, MockExecutor::new())
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
fn missing_apm_reason_recommends_winget_for_wsl() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = make_home_context_for_platform(dir.path(), Platform::new_wsl());

    assert_eq!(
        missing_apm_reason(&ctx),
        "apm not found in PATH; install the Windows package with `winget.exe install \
         Microsoft.APM` and re-open your WSL shell"
    );
}

#[test]
fn missing_apm_reason_recommends_winget_for_windows() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = make_home_context_for_platform(dir.path(), Platform::new(Os::Windows, false));

    assert_eq!(
        missing_apm_reason(&ctx),
        "apm not found in PATH; install it with `winget install Microsoft.APM`"
    );
}

#[test]
fn missing_apm_reason_omits_unknown_install_command() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = make_home_context_for_platform(dir.path(), Platform::new(Os::Linux, false));

    assert_eq!(missing_apm_reason(&ctx), "apm not found in PATH");
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
                ["install", "-g", "--target", "copilot,codex,copilot-app"]
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

#[test]
fn run_includes_copilot_app_target_on_windows_when_app_database_exists() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_home_fragment(
        dir.path(),
        "base.yml",
        "name: base\nversion: 1.0.0\ndependencies:\n  apm:\n    - github/awesome-copilot/plugins/project-planning\n",
    );
    write_copilot_app_db(dir.path());

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
                ["install", "-g", "--target", "copilot,codex,copilot-app"]
            );
            assert!(env.contains(&("GIT_TERMINAL_PROMPT", "0")));
            Ok(ok_result("installed\n"))
        });

    let ctx = make_home_context_for_platform_with_executor(
        dir.path(),
        Platform::new(Os::Windows, false),
        mock,
    );

    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok after apm install, got {result:?}"
    );
}

#[test]
fn run_omits_copilot_app_target_when_app_database_missing() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_home_fragment(
        dir.path(),
        "base.yml",
        "name: base\nversion: 1.0.0\ndependencies:\n  apm:\n    - github/awesome-copilot/plugins/project-planning\n",
    );

    let mut mock = MockExecutor::new();
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(|_| true);
    let install_cwd = dir.path().to_path_buf();
    mock.expect_run_in_with_env()
        .once()
        .returning(move |dir, program, args, env| {
            assert_eq!(dir, install_cwd.as_path());
            assert_eq!(program, "apm");
            assert_eq!(args, ["install", "-g", "--target", "copilot,codex"]);
            assert!(env.contains(&("GIT_TERMINAL_PROMPT", "0")));
            Ok(ok_result("installed\n"))
        });

    let ctx = make_home_context_without_copilot_app_with_executor(dir.path(), mock);

    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok after apm install, got {result:?}"
    );
}

/// Create `<home>/.copilot/data.db` so the `copilot-app` target is available.
fn write_copilot_app_db(home: &Path) {
    let copilot_dir = home.join(".copilot");
    std::fs::create_dir_all(&copilot_dir).expect("create .copilot dir");
    let db_path = copilot_dir.join("data.db");
    std::fs::write(&db_path, b"db").expect("write data.db");
}

#[test]
fn update_skips_apm_update_when_dependencies_current() {
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
    // No `apm update` expectation: when nothing is outdated the update
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
                    "update",
                    "-g",
                    "--yes",
                    "--target",
                    "copilot,codex,copilot-app"
                ]
            );
            assert!(env.contains(&("GIT_TERMINAL_PROMPT", "0")));
            // Simulate a real ref advance by rewriting the lockfile; the task
            // detects change by comparing the lockfile before and after.
            std::fs::write(dir.join(".apm").join("apm.lock.yaml"), "advanced\n")
                .expect("rewrite lock");
            Ok(ok_result("updated\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = UpdateApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok after apm update, got {result:?}"
    );
}

#[test]
fn update_stays_quiet_when_apm_update_reports_no_changes() {
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
                    "update",
                    "-g",
                    "--yes",
                    "--target",
                    "copilot,codex,copilot-app"
                ]
            );
            // The mock leaves the lockfile untouched, so the before/after
            // comparison reports no advance even though `apm update` re-ran.
            Ok(ok_result("  [+] github.com/example/plugin (cached)\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = UpdateApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok when update made no changes, got {result:?}"
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
                ["install", "-g", "--target", "copilot,codex,copilot-app"]
            );
            Ok(ok_result("installed\n"))
        });
    // No `apm outdated` / `apm update` expectations are registered: the
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
    // No `apm outdated` / `apm update` expectations: the converged-manifest
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
                ["install", "-g", "--target", "copilot,codex,copilot-app"]
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
                ["install", "-g", "--target", "copilot,codex,copilot-app"]
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
                ["install", "-g", "--target", "copilot,codex,copilot-app"]
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
fn workflow_encode_detection_accepts_current_apm_wording() {
    let current = "  [x] 4 packages failed:\n    +- dotnet/skills/plugins/dotnet-diag \
                   -- Failed to integrate primitives: Refusing to lockfile-encode non-APM \
                   workflow id: 'optimizing-dotnet-performance.agent.md'\n    +- \
                   dotnet/skills/plugins/dotnet-msbuild -- Failed to integrate primitives: \
                   Refusing to lockfile-encode non-APM workflow id: 'msbuild.agent.md'\n    \
                   +- github/awesome-copilot/plugins/context-engineering -- Failed to \
                   integrate primitives: Refusing to lockfile-encode non-APM workflow id: \
                   'context-architect.agent.md'\n    +- \
                   github/awesome-copilot/plugins/project-planning -- Failed to integrate \
                   primitives: Refusing to lockfile-encode non-APM workflow id: \
                   'implementation-plan.agent.md'\n[!] Installed 14 APM dependencies in 7.9s \
                   with 4 error(s).";
    assert_eq!(tolerable_workflow_encode_failures(current), Some(4));
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
