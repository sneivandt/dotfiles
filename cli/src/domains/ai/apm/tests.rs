//! Unit tests for the APM package install task.

use super::*;
use crate::engine::Context;
use crate::infra::ConfigHandle;
use crate::infra::exec::{ExecError, ExecResult, MockExecutor};
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::{
    assert_task_changed, assert_task_ok, empty_config, make_linux_context, task_skipped,
};

use super::test_fixture::{
    DEFAULT_FRAGMENT, expect_apm_install, expect_apm_install_without_enable, expect_apm_outdated,
    expect_apm_outdated_in_sequence, expect_apm_prune, expect_apm_update,
    expect_copilot_app_enable, expect_copilot_app_workflow_install, expect_copilot_cowork_enable,
    expect_copilot_cowork_install, expect_copilot_experimental_install, expect_which_apm, has_env,
    install_task, make_context_with_home, make_home_context_with_executor, update_task,
    write_copilot_app_db, write_current_manifest_and_lock, write_current_manifest_lock_and_marker,
    write_default_home_fragment, write_home_fragment,
};
use super::update::normalize_lock_snapshot;
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

fn install_task_with_fragment(root: &Path, filename: &str) -> InstallApmPackages {
    InstallApmPackages::new(ConfigHandle::new(vec![ApmFragmentSource::new(
        root.join("symlinks")
            .join("apm")
            .join("config")
            .join(filename),
        filename.into(),
    )]))
}

fn make_home_context_for_platform(home: &Path, platform: Platform) -> Context {
    make_context_with_home(home, platform, MockExecutor::new())
}

fn command_failure(message: &str) -> ExecError {
    ExecError::non_zero("apm", ExecResult::failure("", message, Some(1)))
}

#[test]
fn should_run_false_when_no_fragments() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = make_home_context(dir.path());
    assert!(!install_task().should_run(&ctx));
}

#[test]
fn should_run_true_when_configured_repo_fragment_exists() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_repo_fragment(dir.path(), "team.yaml", "name: test\n");
    let config = empty_config(dir.path().to_path_buf());
    let ctx = make_linux_context(config);
    assert!(install_task_with_fragment(dir.path(), "team.yaml").should_run(&ctx));
}

#[test]
fn should_run_true_when_only_overlay_fragment_in_home() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_home_fragment(dir.path(), "work.yml", "name: work\n");
    let ctx = make_home_context(dir.path());
    assert!(install_task().should_run(&ctx));
}

#[test]
fn run_skips_when_apm_not_found() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_home_fragment(dir.path(), "base.yml", "name: base\n");

    let ctx = make_home_context(dir.path());
    let result = install_task().run(&ctx).expect("run should not error");
    let reason = task_skipped(&result);
    assert!(
        reason.contains("apm not found"),
        "expected reason to mention 'apm not found', got {reason:?}"
    );
}

#[test]
fn missing_apm_reason_cases() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let cases = [
        (
            Platform::new_wsl(),
            "apm not found in PATH; install the Windows package with `winget.exe install \
             Microsoft.APM` and re-open your WSL shell",
        ),
        (
            Platform::new(Os::Windows, false),
            "apm not found in PATH; install it with `winget install Microsoft.APM`",
        ),
        (Platform::new(Os::Linux, false), "apm not found in PATH"),
    ];
    for (platform, expected) in cases {
        let ctx = make_home_context_for_platform(dir.path(), platform);
        assert_eq!(missing_apm_reason(&ctx), expected);
    }
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

    let result = install_task().run(&ctx).expect("run should not error");
    assert_task_changed(&result);
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
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|spec| {
            assert_eq!(spec.arguments(), ["install", "-g"]);
            Ok(ExecResult::success("installed\n"))
        });
    expect_copilot_app_enable(&mut mock, &mut seq);
    expect_copilot_cowork_enable(&mut mock, &mut seq);
    expect_copilot_experimental_install(&mut mock, &mut seq);
    expect_apm_prune(&mut mock, &mut seq, dir.path());

    let ctx = make_context_with_home(dir.path(), Platform::new(Os::Windows, false), mock);

    let result = install_task().run(&ctx).expect("run should not error");
    assert_task_changed(&result);
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
            Ok(ExecResult::success("installed\n"))
        });
    expect_apm_prune(&mut mock, &mut seq, dir.path());

    let ctx = make_context_with_home(dir.path(), Platform::new(Os::Linux, false), mock);

    let result = install_task().run(&ctx).expect("run should not error");
    assert_task_changed(&result);
}

#[test]
fn update_apply_stays_quiet_when_dependencies_are_current() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    expect_apm_outdated_in_sequence(
        &mut mock,
        &mut seq,
        dir.path(),
        "[*] No remote dependencies to check\n",
    );

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = update_task().run(&ctx).expect("run should not error");
    assert_task_ok(&result);
}

#[test]
fn update_advances_dependencies_when_lockfile_changes() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    expect_apm_outdated_in_sequence(
        &mut mock,
        &mut seq,
        dir.path(),
        "[!] 1 outdated dependency found\n",
    );
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
            Ok(ExecResult::success("updated\n"))
        });
    expect_copilot_app_enable(&mut mock, &mut seq);
    expect_copilot_app_workflow_install(&mut mock, &mut seq);

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = update_task().run(&ctx).expect("run should not error");
    assert_task_changed(&result);
}

#[test]
fn update_stays_quiet_when_apm_update_reports_no_changes() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    expect_apm_outdated_in_sequence(
        &mut mock,
        &mut seq,
        dir.path(),
        "[!] 1 outdated dependency found\n",
    );
    // The mock leaves the lockfile untouched, so the before/after comparison
    // reports no advance even though `apm update` re-ran.
    expect_apm_update(
        &mut mock,
        &mut seq,
        "  [+] github.com/example/plugin (cached)\n",
    );
    expect_copilot_app_enable(&mut mock, &mut seq);
    expect_copilot_app_workflow_install(&mut mock, &mut seq);

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = update_task().run(&ctx).expect("run should not error");
    assert_task_ok(&result);
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
    expect_apm_outdated_in_sequence(
        &mut mock,
        &mut seq,
        dir.path(),
        "[!] 1 outdated dependency found\n",
    );
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |spec| {
            assert_eq!(spec.arguments(), ["update", "-g", "--yes"]);
            Ok(ExecResult::success(
                "All dependencies already at their latest matching refs.\n",
            ))
        });
    expect_copilot_app_enable(&mut mock, &mut seq);
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
            Ok(ExecResult::success("installed workflows\n"))
        });

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = update_task().run(&ctx).expect("run should not error");
    assert_task_ok(&result);
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
    expect_apm_outdated_in_sequence(
        &mut mock,
        &mut seq,
        dir.path(),
        "[!] 1 outdated dependency found\n",
    );
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
            Ok(ExecResult::success("updated\n"))
        });
    expect_copilot_app_enable(&mut mock, &mut seq);
    expect_copilot_app_workflow_install(&mut mock, &mut seq);

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = update_task().run(&ctx).expect("run should not error");
    assert_task_changed(&result);
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
    expect_apm_outdated(&mut mock, dir.path(), "[!] 1 outdated dependency found\n");
    let ctx = make_home_context_with_executor(dir.path(), mock);

    let err = update_task()
        .run(&ctx)
        .expect_err("non-NotFound lockfile read failures should propagate");
    assert!(
        format!("{err:#}").contains("reading APM lockfile"),
        "expected lockfile read context, got {err:#}"
    );
}

#[test]
fn update_ignores_deployment_ledger_rewrites() {
    let before = br"
lockfile_version: '1'
dependencies:
- repo_url: example/plugin
  resolved_commit: abc123
  deployed_files:
  - copilot-app://workflows/example
  deployed_file_hashes:
    copilot-app://workflows/example: old
deployments:
  example/plugin:
  - copilot-app://workflows/example
mcp_servers:
  copilot-app:
  - example
";
    let after = br"
lockfile_version: '1'
generated_at: '2026-07-25T16:29:53.449377+00:00'
dependencies:
- repo_url: example/plugin
  resolved_commit: abc123
  deployed_files:
  - cowork://skills/example/SKILL.md
  deployed_file_hashes:
    cowork://skills/example/SKILL.md: new
deployments:
  example/plugin:
  - cowork://skills/example/SKILL.md
mcp_configs:
  copilot-app:
  - example
mcp_target_servers:
  copilot-app:
  - example
";

    assert_eq!(
        normalize_lock_snapshot(&String::from_utf8_lossy(before)),
        normalize_lock_snapshot(&String::from_utf8_lossy(after))
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

    let result = install_task().run(&ctx).expect("run should not error");
    assert_task_ok(&result);
}

#[test]
fn dry_run_uses_managed_fragment_source_when_home_link_is_missing() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());
    write_repo_fragment(dir.path(), "base.yml", DEFAULT_FRAGMENT);
    std::fs::remove_file(dir.path().join(".apm").join("config").join("base.yml"))
        .expect("remove managed home fragment");

    let task = install_task_with_fragment(dir.path(), "base.yml");
    let ctx = make_home_context(dir.path()).with_dry_run(true);

    let result = task.run(&ctx).expect("run should not error");

    assert_task_ok(&result);
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

    let result = install_task().run(&ctx).expect("run should not error");
    assert_task_changed(&result);
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

    let result = install_task().run(&ctx).expect("run should not error");
    assert_task_changed(&result);
}

#[test]
fn install_deploys_copilot_cowork_separately_on_windows() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_install_home_fragment(dir.path());

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|spec| {
            assert_eq!(spec.arguments(), ["install", "-g"]);
            Ok(ExecResult::success("installed\n"))
        });
    expect_copilot_cowork_enable(&mut mock, &mut seq);
    expect_copilot_cowork_install(&mut mock, &mut seq);
    expect_apm_prune(&mut mock, &mut seq, dir.path());

    let ctx = make_context_with_home(dir.path(), Platform::new(Os::Windows, false), mock);
    let result = install_task().run(&ctx).expect("run should not error");

    assert_task_changed(&result);
}

#[test]
fn install_skips_cowork_enable_when_apm_config_reports_it_enabled() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_install_home_fragment(dir.path());
    write_apm_config(dir.path(), "{\"experimental\":{\"copilot_cowork\":true}}");

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|spec| {
            assert_eq!(spec.arguments(), ["install", "-g"]);
            Ok(ExecResult::success("installed\n"))
        });
    expect_copilot_cowork_install(&mut mock, &mut seq);
    expect_apm_prune(&mut mock, &mut seq, dir.path());

    let ctx = make_context_with_home(dir.path(), Platform::new(Os::Windows, false), mock);
    let result = install_task().run(&ctx).expect("run should not error");

    assert_task_changed(&result);
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

        let result = install_task().run(&ctx).expect("run should not error");
        assert_task_changed(&result);
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

    let result = update_task().run(&ctx).expect("run should not error");
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

    let result = install_task().run(&ctx).expect("run should not error");
    assert_task_changed(&result);
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

    let result = install_task().run(&ctx).expect("run should not error");
    assert_task_changed(&result);
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

    let result = install_task().run(&ctx).expect("run should not error");
    assert_task_ok(&result);
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

    let result = update_task().run(&ctx).expect("run should not error");
    assert_task_ok(&result);
}

#[test]
fn update_dry_run_reports_discovered_dependency_advancement() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_lock_and_marker(dir.path());

    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    expect_apm_outdated(&mut mock, dir.path(), "[!] 2 outdated dependencies found\n");
    let ctx = make_home_context_with_executor(dir.path(), mock).with_dry_run(true);

    let result = update_task().run(&ctx).expect("run should not error");
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

    let result = update_task().run(&ctx).expect("run should not error");
    assert!(
        matches!(result, TaskResult::Skipped(_)),
        "expected Skipped when apm is not on PATH, got {result:?}"
    );
}

/// Run install with an `apm install -g` mock that fails with `message`.
/// Both the auth-skip and generic-propagate paths share this setup and differ
/// only downstream.
fn run_install_after_apm_error(message: &'static str) -> anyhow::Result<TaskResult> {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(move |_| Err(command_failure(message)));

    let ctx = make_home_context_with_executor(dir.path(), mock);
    install_task().run(&ctx)
}

#[test]
fn run_skips_auth_failures() {
    let result =
        run_install_after_apm_error("fatal: Authentication failed; terminal prompts disabled")
            .expect("auth failure should skip");
    let reason = task_skipped(&result);
    assert!(
        reason.contains("GitHub authentication"),
        "expected auth skip reason, got {reason:?}"
    );
}

#[test]
fn run_propagates_non_auth_apm_failures() {
    let err = run_install_after_apm_error("archive extraction failed")
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
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|spec| {
            assert_eq!(spec.arguments(), ["install", "-g"]);
            Ok(ExecResult::success("installed\n"))
        });
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
    expect_copilot_app_workflow_install(&mut mock, &mut seq);
    expect_apm_prune(&mut mock, &mut seq, dir.path());

    let ctx = make_home_context_with_executor(dir.path(), mock);

    let result = install_task()
        .run(&ctx)
        .expect("install should continue despite enable failure");
    assert_task_changed(&result);
}

#[test]
fn run_propagates_prune_failures_after_persisting_marker() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|spec| {
            assert_eq!(spec.arguments(), ["install", "-g"]);
            Ok(ExecResult::success("installed\n"))
        });
    expect_copilot_app_enable(&mut mock, &mut seq);
    expect_copilot_app_workflow_install(&mut mock, &mut seq);
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
    let err = install_task()
        .run(&ctx)
        .expect_err("prune failures should propagate");
    assert!(
        format!("{err:#}").contains("prune failed"),
        "expected propagated prune failure, got {err:#}"
    );
}

#[test]
fn auth_failure_detection_cases() {
    let cases = [
        (
            "git failed: HTTP 403 Forbidden while fetching repository",
            true,
        ),
        (
            "fatal: Authentication failed; terminal prompts disabled",
            true,
        ),
        (
            "credential cache cleanup failed after archive extraction",
            false,
        ),
    ];
    for (message, expected) in cases {
        assert_eq!(
            looks_like_auth_failure(message),
            expected,
            "message: {message}"
        );
    }
}

#[test]
fn run_propagates_copilot_app_install_failures() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|spec| {
            assert_eq!(spec.arguments(), ["install", "-g"]);
            Ok(ExecResult::success("installed\n"))
        });
    expect_copilot_app_enable(&mut mock, &mut seq);
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

    let err = install_task()
        .run(&ctx)
        .expect_err("the copilot-app install must fail closed");
    assert!(
        format!("{err:#}").contains("apm install failed"),
        "expected propagated copilot-app failure, got {err:#}"
    );
}
