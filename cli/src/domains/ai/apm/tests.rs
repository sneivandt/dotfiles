//! Unit tests for the APM package install task.

use super::*;
use crate::engine::Context;
use crate::infra::exec::{CommandSpec, ExecError, ExecResult, MockExecutor};
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::{empty_config, make_linux_context};

use super::test_fixture::{
    make_context_with_home, ok_result, write_copilot_app_db, write_current_manifest_and_lock,
    write_current_manifest_lock_and_marker, write_default_home_fragment, write_home_fragment,
};
use std::path::PathBuf;

const INSTALL_FIXTURE_FRAGMENT: &str = "name: base\nversion: 1.0.0\ndependencies:\n  apm:\n    - github/awesome-copilot/plugins/project-planning\n";
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

fn write_install_home_fragment(home: &Path) {
    write_home_fragment(home, "base.yml", INSTALL_FIXTURE_FRAGMENT);
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
    make_context_with_home(home, platform, executor)
}

fn make_home_context_for_platform(home: &Path, platform: Platform) -> Context {
    make_home_context_for_platform_with_executor(home, platform, MockExecutor::new())
}

fn expect_which_apm(mock: &mut MockExecutor, found: bool) {
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(move |_| found);
}

fn has_env(spec: &CommandSpec, key: &str, value: &str) -> bool {
    spec.environment()
        .iter()
        .any(|(actual_key, actual_value)| actual_key == key && actual_value == value)
}

fn command_failure(message: &str) -> ExecError {
    ExecError::non_zero(
        "apm",
        ExecResult {
            stdout: String::new(),
            stderr: message.to_string(),
            success: false,
            code: Some(1),
        },
    )
}

fn expect_copilot_app_enable(mock: &mut MockExecutor, seq: &mut mockall::Sequence) {
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(|spec| {
            assert_eq!(spec.arguments(), ["experimental", "enable", "copilot-app"]);
            assert!(has_env(&spec, "GIT_TERMINAL_PROMPT", "0"));
            Ok(ok_result("[!] copilot-app is already enabled.\n"))
        });
}

fn expect_apm_prune(mock: &mut MockExecutor, seq: &mut mockall::Sequence, cwd: &Path) {
    let prune_cwd = cwd.join(".apm");
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(move |spec| {
            assert_eq!(spec.working_dir(), Some(prune_cwd.as_path()));
            assert_eq!(spec.program(), "apm");
            assert_eq!(spec.arguments(), ["prune"]);
            assert!(has_env(&spec, "GIT_TERMINAL_PROMPT", "0"));
            Ok(ok_result("pruned\n"))
        });
}

fn expect_apm_install(mock: &mut MockExecutor, seq: &mut mockall::Sequence, cwd: &Path) {
    expect_copilot_app_enable(mock, seq);
    expect_apm_install_without_enable(mock, seq, cwd);
}

/// Expect the install/deploy/prune calls without `apm experimental enable`.
fn expect_apm_install_without_enable(
    mock: &mut MockExecutor,
    seq: &mut mockall::Sequence,
    cwd: &Path,
) {
    let install_cwd = cwd.to_path_buf();
    let copilot_app_cwd = install_cwd.clone();
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(move |spec| {
            assert_eq!(spec.working_dir(), Some(copilot_app_cwd.as_path()));
            assert_eq!(spec.program(), "apm");
            assert_eq!(spec.arguments(), ["install", "-g"]);
            assert!(has_env(&spec, "GIT_TERMINAL_PROMPT", "0"));
            assert!(has_env(&spec, "GCM_INTERACTIVE", "Never"));
            assert!(has_env(&spec, "GCM_GUI_PROMPT", "false"));
            Ok(ok_result("installed\n"))
        });
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(move |spec| {
            assert_eq!(spec.working_dir(), Some(install_cwd.as_path()));
            assert_eq!(spec.program(), "apm");
            assert_eq!(
                spec.arguments(),
                ["install", "-g", "--target", "copilot-app"]
            );
            assert!(has_env(&spec, "GIT_TERMINAL_PROMPT", "0"));
            Ok(ok_result("installed workflows\n"))
        });
    expect_apm_prune(mock, seq, cwd);
}

fn expect_apm_outdated(mock: &mut MockExecutor, cwd: &Path, stdout: &'static str) {
    let outdated_cwd = cwd.to_path_buf();
    mock.expect_execute().once().returning(move |spec| {
        assert_eq!(spec.working_dir(), Some(outdated_cwd.as_path()));
        assert_eq!(spec.program(), "apm");
        assert_eq!(spec.arguments(), ["outdated", "-g"]);
        assert!(has_env(&spec, "GIT_TERMINAL_PROMPT", "0"));
        assert!(has_env(&spec, "GCM_INTERACTIVE", "Never"));
        assert!(has_env(&spec, "GCM_GUI_PROMPT", "false"));
        Ok(ok_result(stdout))
    });
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
        other @ (TaskResult::Ok
        | TaskResult::DryRun
        | TaskResult::CheckPassed
        | TaskResult::NotApplicable(_)
        | TaskResult::Failed(_)
        | TaskResult::Batch(_)) => {
            panic!("expected TaskResult::Skipped, got {other:?}")
        }
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
    write_install_home_fragment(dir.path());

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    expect_apm_install(&mut mock, &mut seq, dir.path());

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() > 0),
        "expected changed result after apm install, got {result:?}"
    );
    let manifest = std::fs::read_to_string(dir.path().join(".apm").join("apm.yml"))
        .expect("read merged manifest");
    assert!(manifest.contains("github/awesome-copilot/plugins/project-planning"));
}

#[test]
fn run_installs_copilot_app_separately_on_windows_when_app_database_exists() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_install_home_fragment(dir.path());
    write_copilot_app_db(dir.path());

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    expect_apm_install(&mut mock, &mut seq, dir.path());

    let ctx = make_home_context_for_platform_with_executor(
        dir.path(),
        Platform::new(Os::Windows, false),
        mock,
    );

    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() > 0),
        "expected changed result after apm install, got {result:?}"
    );
}

#[test]
fn run_uses_runtime_auto_detection_when_copilot_app_database_missing() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_install_home_fragment(dir.path());

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    let install_cwd = dir.path().to_path_buf();
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |spec| {
            assert_eq!(spec.working_dir(), Some(install_cwd.as_path()));
            assert_eq!(spec.program(), "apm");
            assert_eq!(spec.arguments(), ["install", "-g"]);
            assert!(has_env(&spec, "GIT_TERMINAL_PROMPT", "0"));
            Ok(ok_result("installed\n"))
        });
    expect_apm_prune(&mut mock, &mut seq, dir.path());

    let ctx = make_home_context_without_copilot_app_with_executor(dir.path(), mock);

    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() > 0),
        "expected changed result after apm install, got {result:?}"
    );
}

#[test]
fn update_runs_native_update_when_dependencies_current() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    let update_cwd = dir.path().to_path_buf();
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |spec| {
            assert_eq!(spec.working_dir(), Some(update_cwd.as_path()));
            assert_eq!(spec.program(), "apm");
            assert_eq!(spec.arguments(), ["update", "-g", "--yes"]);
            assert!(has_env(&spec, "GIT_TERMINAL_PROMPT", "0"));
            Ok(ok_result("already current\n"))
        });
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |spec| {
            assert_eq!(spec.program(), "apm");
            assert_eq!(
                spec.arguments(),
                ["install", "-g", "--target", "copilot-app"]
            );
            Ok(ok_result("installed workflows\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = UpdateApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok when dependencies are already current, got {result:?}"
    );
}

#[test]
fn update_advances_dependencies_when_lockfile_changes() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    let update_cwd = dir.path().to_path_buf();
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |spec| {
            assert_eq!(spec.working_dir(), Some(update_cwd.as_path()));
            assert_eq!(spec.program(), "apm");
            assert_eq!(spec.arguments(), ["update", "-g", "--yes"]);
            assert!(has_env(&spec, "GIT_TERMINAL_PROMPT", "0"));
            // Simulate a real ref advance by rewriting the lockfile; the task
            // detects change by comparing the lockfile before and after.
            std::fs::write(
                spec.working_dir()
                    .expect("APM update working directory")
                    .join(".apm")
                    .join("apm.lock.yaml"),
                "advanced\n",
            )
            .expect("rewrite lock");
            Ok(ok_result("updated\n"))
        });
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |spec| {
            assert_eq!(spec.program(), "apm");
            assert_eq!(
                spec.arguments(),
                ["install", "-g", "--target", "copilot-app"]
            );
            Ok(ok_result("installed workflows\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = UpdateApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() > 0),
        "expected changed result after apm update, got {result:?}"
    );
}

#[test]
fn update_stays_quiet_when_apm_update_reports_no_changes() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |spec| {
            assert_eq!(spec.arguments(), ["update", "-g", "--yes"]);
            // The mock leaves the lockfile untouched, so the before/after
            // comparison reports no advance even though `apm update` re-ran.
            Ok(ok_result("  [+] github.com/example/plugin (cached)\n"))
        });
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |spec| {
            assert_eq!(
                spec.arguments(),
                ["install", "-g", "--target", "copilot-app"]
            );
            Ok(ok_result("installed workflows\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = UpdateApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok when update made no changes, got {result:?}"
    );
}

/// Build a realistic APM lockfile whose only variable is the `generated_at`
/// bookkeeping timestamp that apm stamps on every serialization.
fn lock_with_timestamp(stamp: &str) -> String {
    format!(
        "lockfile_version: '1'\ngenerated_at: '{stamp}'\napm_version: 0.26.0\ndependencies:\n- \
         repo_url: example/plugin\n  resolved_commit: abc123\n"
    )
}

#[test]
fn update_ignores_lockfile_timestamp_rewrites() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());
    std::fs::write(
        dir.path().join(".apm").join("apm.lock.yaml"),
        lock_with_timestamp("2026-07-25T16:28:24.149463+00:00"),
    )
    .expect("write realistic lock");

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |spec| {
            assert_eq!(spec.arguments(), ["update", "-g", "--yes"]);
            Ok(ok_result(
                "All dependencies already at their latest matching refs.\n",
            ))
        });
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |spec| {
            assert_eq!(
                spec.arguments(),
                ["install", "-g", "--target", "copilot-app"]
            );
            // apm re-serializes the lockfile on every write, stamping a fresh
            // `generated_at` even when no dependency ref advanced.
            std::fs::write(
                spec.working_dir()
                    .expect("APM install working directory")
                    .join(".apm")
                    .join("apm.lock.yaml"),
                lock_with_timestamp("2026-07-25T16:29:53.449377+00:00"),
            )
            .expect("rewrite lock");
            Ok(ok_result("installed workflows\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = UpdateApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok when only the lockfile timestamp changed, got {result:?}"
    );
}

#[test]
fn update_reports_change_when_resolved_ref_advances_alongside_timestamp() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());
    std::fs::write(
        dir.path().join(".apm").join("apm.lock.yaml"),
        lock_with_timestamp("2026-07-25T16:28:24.149463+00:00"),
    )
    .expect("write realistic lock");

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |spec| {
            assert_eq!(spec.arguments(), ["update", "-g", "--yes"]);
            std::fs::write(
                spec.working_dir()
                    .expect("APM update working directory")
                    .join(".apm")
                    .join("apm.lock.yaml"),
                lock_with_timestamp("2026-07-25T16:29:53.449377+00:00").replace("abc123", "def456"),
            )
            .expect("rewrite lock");
            Ok(ok_result("updated\n"))
        });
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |spec| {
            assert_eq!(
                spec.arguments(),
                ["install", "-g", "--target", "copilot-app"]
            );
            Ok(ok_result("installed workflows\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = UpdateApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() > 0),
        "expected changed result when a resolved ref advanced, got {result:?}"
    );
}

#[test]
fn update_propagates_lockfile_read_failures() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());
    let lock_path = dir.path().join(".apm").join("apm.lock.yaml");
    std::fs::remove_file(&lock_path).expect("remove lockfile");
    std::fs::create_dir(&lock_path).expect("create lockfile-shaped directory");

    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    let ctx = make_home_context_with_executor(dir.path(), mock);

    let err = UpdateApmPackages
        .run(&ctx)
        .expect_err("non-NotFound lockfile read failures should propagate");
    assert!(
        format!("{err:#}").contains("reading APM lockfile"),
        "expected lockfile read context, got {err:#}"
    );
}

#[test]
fn install_task_skips_apm_when_manifest_sources_and_targets_are_unchanged() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());

    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    // No `apm` expectations at all: a converged tree must not spawn `apm
    // install`, `apm install --target copilot-app`, `apm prune`, or `apm
    // experimental enable`.  The mock panics on any unexpected call.

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok when nothing APM cares about changed, got {result:?}"
    );
}

#[test]
fn install_task_redeploys_when_a_local_plugin_source_changes() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let plugin = seed_local_plugin(dir.path());

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    expect_apm_install(&mut mock, &mut seq, dir.path());

    let ctx = make_home_context_with_executor(dir.path(), mock);

    // Editing the symlinked plugin must invalidate the marker even though the
    // manifest text is byte-identical.
    std::fs::write(plugin.join("skill.md"), "v2\n").expect("edit plugin file");

    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() > 0),
        "expected a redeploy after a local plugin edit, got {result:?}"
    );
}

/// Seed a converged home whose manifest declares one local plugin source, and
/// return that plugin's directory.  Editing a file inside it is the cheapest
/// way to force a redeploy without touching the manifest text.
fn seed_local_plugin(home: &Path) -> PathBuf {
    let plugin = home.join(".apm").join("plugins").join("local");
    std::fs::create_dir_all(&plugin).expect("create plugin dir");
    std::fs::write(plugin.join("skill.md"), "v1\n").expect("write plugin file");
    write_home_fragment(
        home,
        "local.yml",
        "name: local\nversion: 1.0.0\ndependencies:\n  apm:\n    - ~/.apm/plugins/local\n",
    );
    write_current_manifest_lock_and_marker(home);
    plugin
}

/// Write apm's own config file, the source the enable fast path reads.
fn write_apm_config(home: &Path, contents: &str) {
    let apm_dir = home.join(".apm");
    std::fs::create_dir_all(&apm_dir).expect("create ~/.apm");
    std::fs::write(apm_dir.join("config.json"), contents).expect("write apm config");
}

#[test]
fn install_skips_experimental_enable_when_apm_config_reports_it_enabled() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let plugin = seed_local_plugin(dir.path());
    write_apm_config(
        dir.path(),
        "{\"default_client\":\"vscode\",\"experimental\":{\"copilot_app\":true}}",
    );

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    // No `apm experimental enable` expectation: the flag is already recorded in
    // apm's config, so re-asserting it would only cost a process start.
    expect_apm_install_without_enable(&mut mock, &mut seq, dir.path());

    let ctx = make_home_context_with_executor(dir.path(), mock);
    std::fs::write(plugin.join("skill.md"), "v2\n").expect("edit plugin file");

    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() > 0),
        "expected a redeploy after a local plugin edit, got {result:?}"
    );
}

#[test]
fn install_runs_experimental_enable_when_apm_config_is_unusable() {
    for contents in ["not json at all", "{}", "{\"experimental\":{}}"] {
        let dir = tempfile::tempdir().expect("create temp dir");
        let plugin = seed_local_plugin(dir.path());
        write_apm_config(dir.path(), contents);

        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        expect_which_apm(&mut mock, true);
        // Anything the config cannot answer falls through to the authoritative
        // idempotent CLI call rather than assuming the flag is set.
        expect_apm_install(&mut mock, &mut seq, dir.path());

        let ctx = make_home_context_with_executor(dir.path(), mock);
        std::fs::write(plugin.join("skill.md"), "v2\n").expect("edit plugin file");

        let result = InstallApmPackages.run(&ctx).expect("run should not error");
        assert!(
            matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() > 0),
            "expected a redeploy for config {contents:?}, got {result:?}"
        );
    }
}

#[test]
fn update_skips_advancement_when_install_marker_missing() {
    let dir = tempfile::tempdir().expect("create temp dir");
    // Manifest + lock present but no success marker => the current manifest was
    // never installed successfully, so the update task must NOT contact apm.
    write_current_manifest_and_lock(dir.path());

    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    // No `apm outdated` / `apm update` expectations: the converged-manifest
    // guard must short-circuit before any lockfile-advancing call.  The mock
    // panics on any unexpected `execute`.

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
    expect_which_apm(&mut mock, true);
    expect_apm_install(&mut mock, &mut seq, dir.path());

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() > 0),
        "expected changed result after installing unmarked manifest, got {result:?}"
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
        matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() > 0),
        "expected quantified planned work with fragments present, got {result:?}"
    );
    assert!(
        !dir.path().join(".apm").join("apm.yml").exists(),
        "dry-run must not write the generated manifest"
    );
}

#[test]
fn run_dry_run_is_silent_when_apm_install_state_is_current() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());

    let ctx = make_home_context(dir.path()).with_dry_run(true);

    let result = InstallApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected no planned install work, got {result:?}"
    );
}

#[test]
fn update_dry_run_is_silent_when_dependencies_are_current() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());

    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    expect_apm_outdated(
        &mut mock,
        dir.path(),
        "[*] All dependencies are up-to-date\n",
    );

    let ctx = make_home_context_with_executor(dir.path(), mock).with_dry_run(true);

    let result = UpdateApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Ok),
        "expected no planned update work, got {result:?}"
    );
}

#[test]
fn update_dry_run_reports_discovered_dependency_advancement() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());

    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    expect_apm_outdated(&mut mock, dir.path(), "[!] 2 outdated dependencies found\n");
    let ctx = make_home_context_with_executor(dir.path(), mock).with_dry_run(true);

    let result = UpdateApmPackages.run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::DryRun),
        "expected an update plan for discovered outdated dependencies, got {result:?}"
    );
}

#[test]
fn update_skips_when_apm_not_found() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());

    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, false);
    // No `execute` expectations: a missing apm binary must short-circuit
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
    expect_which_apm(&mut mock, true);
    expect_copilot_app_enable(&mut mock, &mut seq);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|_| {
            Err(command_failure(
                "fatal: Authentication failed; terminal prompts disabled",
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
        other @ (TaskResult::Ok
        | TaskResult::DryRun
        | TaskResult::CheckPassed
        | TaskResult::NotApplicable(_)
        | TaskResult::Failed(_)
        | TaskResult::Batch(_)) => {
            panic!("expected TaskResult::Skipped, got {other:?}")
        }
    }
}

#[test]
fn run_propagates_non_auth_apm_failures() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    expect_copilot_app_enable(&mut mock, &mut seq);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|_| Err(command_failure("archive extraction failed")));

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
    expect_which_apm(&mut mock, true);
    // A best-effort experimental-enable failure (e.g. an older apm without
    // the `experimental` subcommand) must never abort the install.
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|spec| {
            assert_eq!(spec.arguments(), ["experimental", "enable", "copilot-app"]);
            Err(command_failure(
                "error: unrecognized subcommand 'experimental'",
            ))
        });
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|spec| {
            assert_eq!(spec.arguments(), ["install", "-g"]);
            Ok(ok_result("installed\n"))
        });
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|spec| {
            assert_eq!(
                spec.arguments(),
                ["install", "-g", "--target", "copilot-app"]
            );
            Ok(ok_result("installed workflows\n"))
        });
    expect_apm_prune(&mut mock, &mut seq, dir.path());

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = InstallApmPackages
        .run(&ctx)
        .expect("install should continue despite enable failure");
    assert!(matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() > 0));
}

#[test]
fn run_propagates_prune_failures_after_persisting_marker() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    expect_copilot_app_enable(&mut mock, &mut seq);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|spec| {
            assert_eq!(spec.arguments(), ["install", "-g"]);
            Ok(ok_result("installed\n"))
        });
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|spec| {
            assert_eq!(
                spec.arguments(),
                ["install", "-g", "--target", "copilot-app"]
            );
            Ok(ok_result("installed workflows\n"))
        });
    let marker = dir.path().join(".apm").join(".dotfiles-manifest.sha256");
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |spec| {
            assert_eq!(
                spec.working_dir()
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str()),
                Some(".apm")
            );
            assert_eq!(spec.arguments(), ["prune"]);
            assert!(marker.exists(), "marker must be persisted before pruning");
            Err(command_failure("prune failed"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);
    let err = InstallApmPackages
        .run(&ctx)
        .expect_err("prune failures should propagate");
    assert!(
        format!("{err:#}").contains("prune failed"),
        "expected propagated prune failure, got {err:#}"
    );
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

#[test]
fn run_propagates_copilot_app_install_failures() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    expect_copilot_app_enable(&mut mock, &mut seq);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|spec| {
            assert_eq!(spec.arguments(), ["install", "-g"]);
            Ok(ok_result("installed\n"))
        });
    // The separate experimental copilot-app deploy must fail closed too.
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|spec| {
            assert_eq!(
                spec.arguments(),
                ["install", "-g", "--target", "copilot-app"]
            );
            Err(command_failure(
                "apm install failed (exit 1): stdout: [!] Installed 1 APM dependencies \
                 with 1 error(s).",
            ))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let err = InstallApmPackages
        .run(&ctx)
        .expect_err("the copilot-app install must fail closed");
    assert!(
        format!("{err:#}").contains("apm install failed"),
        "expected propagated copilot-app failure, got {err:#}"
    );
}
