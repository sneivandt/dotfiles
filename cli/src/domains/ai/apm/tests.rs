//! Focused tests for native APM install/update ownership.

use std::path::Path;

use super::test_fixture::{
    expect_apm_install_without_enable, expect_copilot_app_enable,
    expect_copilot_app_workflow_install, expect_cowork_enable, expect_which_apm, has_env,
    install_task, make_context_with_home, make_windows_cowork_context, update_task,
    write_copilot_app_db, write_current_manifest_and_lock, write_default_home_fragment,
};
use super::*;
use crate::engine::Context;
use crate::infra::exec::{ExecError, ExecResult, MockExecutor};
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::{assert_task_changed, assert_task_ok, task_skipped};

fn command_failure(message: &str) -> ExecError {
    ExecError::non_zero("apm", ExecResult::failure("", message, Some(1)))
}

fn linux_context(home: &Path, executor: MockExecutor) -> Context {
    make_context_with_home(home, Platform::new(Os::Linux, false), executor)
}

fn expect_primary_install(mock: &mut MockExecutor, seq: &mut mockall::Sequence, home: &Path) {
    let home = home.to_path_buf();
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(move |spec| {
            assert_eq!(spec.working_dir(), Some(home.as_path()));
            assert_eq!(spec.program(), "apm");
            assert_eq!(spec.arguments(), ["install", "-g"]);
            assert!(has_env(&spec, "GIT_TERMINAL_PROMPT", "0"));
            Ok(ExecResult::success("installed\n"))
        });
}

fn expect_primary_update(mock: &mut MockExecutor, seq: &mut mockall::Sequence, home: &Path) {
    let home = home.to_path_buf();
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(move |spec| {
            assert_eq!(spec.working_dir(), Some(home.as_path()));
            assert_eq!(spec.program(), "apm");
            assert_eq!(spec.arguments(), ["update", "-g", "--yes"]);
            Ok(ExecResult::success(
                "All dependencies already at their latest matching refs.\n",
            ))
        });
}

#[test]
fn should_run_tracks_effective_fragments() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = linux_context(dir.path(), MockExecutor::new());
    assert!(!install_task().should_run(&ctx));

    write_default_home_fragment(dir.path());
    assert!(install_task().should_run(&ctx));
    assert!(update_task().should_run(&ctx));
}

#[test]
fn install_skips_when_apm_is_missing() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, false);
    let ctx = linux_context(dir.path(), mock);

    let result = install_task().run(&ctx).expect("run install");

    assert!(task_skipped(&result).contains("apm not found"));
}

#[test]
fn install_always_delegates_but_reports_unchanged_lock_as_ok() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_and_lock(dir.path());
    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    expect_primary_install(&mut mock, &mut seq, dir.path());
    let ctx = linux_context(dir.path(), mock);

    let result = install_task().run(&ctx).expect("run install");

    assert_task_ok(&result);
}

#[test]
fn install_reports_manifest_and_lock_changes() {
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
            std::fs::write(
                spec.working_dir()
                    .expect("home")
                    .join(".apm")
                    .join("apm.lock.yaml"),
                "dependencies:\n- repo_url: example/plugin\n",
            )
            .expect("write lock");
            Ok(ExecResult::success("installed\n"))
        });
    let ctx = linux_context(dir.path(), mock);

    let result = install_task().run(&ctx).expect("run install");

    assert_task_changed(&result);
    assert!(dir.path().join(".apm").join("apm.yml").is_file());
    assert!(
        !dir.path()
            .join(".apm")
            .join(".dotfiles-manifest.sha256")
            .exists()
    );
}

#[test]
fn install_uses_native_copilot_app_target() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());
    write_copilot_app_db(dir.path());
    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    expect_primary_install(&mut mock, &mut seq, dir.path());
    expect_copilot_app_enable(&mut mock, &mut seq);
    expect_copilot_app_workflow_install(&mut mock, &mut seq);
    let ctx = linux_context(dir.path(), mock);

    assert_task_changed(&install_task().run(&ctx).expect("run install"));
}

#[test]
fn install_skips_enable_when_apm_config_has_flag() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());
    write_copilot_app_db(dir.path());
    std::fs::write(
        dir.path().join(".apm").join("config.json"),
        "{\"experimental\":{\"copilot_app\":true}}",
    )
    .expect("write APM config");
    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    expect_apm_install_without_enable(&mut mock, &mut seq, dir.path());
    let ctx = linux_context(dir.path(), mock);

    assert_task_changed(&install_task().run(&ctx).expect("run install"));
}

#[test]
fn install_configures_cowork_but_reconciles_files_without_native_target() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());
    let source_skill = dir.path().join(".agents").join("skills").join("example");
    std::fs::create_dir_all(&source_skill).expect("create source skill");
    std::fs::write(source_skill.join("SKILL.md"), "current").expect("write source skill");
    let cowork_skill = dir
        .path()
        .join("OneDrive - Test")
        .join("Documents")
        .join("Cowork")
        .join("skills")
        .join("example");
    std::fs::create_dir_all(&cowork_skill).expect("create Cowork skill");
    std::fs::write(cowork_skill.join("placeholder.txt"), "preserve")
        .expect("write Cowork placeholder");

    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|spec| {
            assert_eq!(spec.arguments(), ["install", "-g"]);
            std::fs::write(
                spec.working_dir()
                    .expect("home")
                    .join(".apm")
                    .join("apm.lock.yaml"),
                "dependencies:\n  - deployed_files:\n      - \
                 .agents/skills/example/SKILL.md\n    target_subset: [agent-skills, \
                 copilot-cowork]\n",
            )
            .expect("write lock");
            Ok(ExecResult::success("installed\n"))
        });
    expect_cowork_enable(&mut mock, &mut seq);
    let ctx = make_windows_cowork_context(dir.path(), mock);

    assert_task_changed(&install_task().run(&ctx).expect("run install"));
    assert_eq!(
        std::fs::read_to_string(cowork_skill.join("SKILL.md")).expect("read Cowork skill"),
        "current"
    );
    assert!(cowork_skill.join("placeholder.txt").is_file());
}

#[test]
fn install_removes_legacy_cowork_records_without_a_detectable_cowork_path() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_and_lock(dir.path());
    std::fs::write(
        dir.path().join(".apm").join("apm.lock.yaml"),
        "dependencies:\n  - deployed_files:\n      - \
         cowork://skills/example/SKILL.md\n      - .agents/skills/example/SKILL.md\n",
    )
    .expect("write legacy Cowork lock");
    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|spec| {
            assert_eq!(spec.arguments(), ["install", "-g"]);
            let lock = std::fs::read_to_string(
                spec.working_dir()
                    .expect("home")
                    .join(".apm")
                    .join("apm.lock.yaml"),
            )
            .expect("read sanitized lock");
            assert!(!lock.contains("cowork://"));
            Ok(ExecResult::success("installed\n"))
        });
    let ctx = linux_context(dir.path(), mock);

    assert_task_changed(&install_task().run(&ctx).expect("run install"));
}

#[test]
fn update_delegates_directly_and_reports_exact_lock_changes() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_and_lock(dir.path());
    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|spec| {
            assert_eq!(spec.arguments(), ["update", "-g", "--yes"]);
            std::fs::write(
                spec.working_dir()
                    .expect("home")
                    .join(".apm")
                    .join("apm.lock.yaml"),
                "dependencies:\n- resolved_commit: advanced\n",
            )
            .expect("advance lock");
            Ok(ExecResult::success("updated\n"))
        });
    let ctx = linux_context(dir.path(), mock);

    assert_task_changed(&update_task().run(&ctx).expect("run update"));
}

#[test]
fn update_reports_ok_when_lock_is_byte_identical() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_and_lock(dir.path());
    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    expect_primary_update(&mut mock, &mut seq, dir.path());
    let ctx = linux_context(dir.path(), mock);

    assert_task_ok(&update_task().run(&ctx).expect("run update"));
}

#[test]
fn update_dry_run_uses_native_apm_plan() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_and_lock(dir.path());
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    mock.expect_execute().once().returning(|spec| {
        assert_eq!(spec.arguments(), ["update", "-g", "--dry-run"]);
        Ok(ExecResult::success("would update example/plugin\n"))
    });
    let ctx = linux_context(dir.path(), mock).with_dry_run(true);

    assert!(matches!(
        update_task().run(&ctx).expect("preview update"),
        TaskResult::DryRun
    ));
}

#[test]
fn install_dry_run_does_not_write_generated_manifest() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());
    let ctx = linux_context(dir.path(), MockExecutor::new()).with_dry_run(true);

    assert_task_changed(&install_task().run(&ctx).expect("preview install"));
    assert!(!dir.path().join(".apm").join("apm.yml").exists());
}

#[test]
fn install_classifies_auth_failures_but_propagates_other_failures() {
    for (message, auth_failure) in [
        (
            "fatal: Authentication failed; terminal prompts disabled",
            true,
        ),
        ("archive extraction failed", false),
    ] {
        let dir = tempfile::tempdir().expect("create temp dir");
        write_default_home_fragment(dir.path());
        let mut mock = MockExecutor::new();
        expect_which_apm(&mut mock, true);
        mock.expect_execute()
            .once()
            .returning(move |_| Err(command_failure(message)));
        let ctx = linux_context(dir.path(), mock);
        let result = install_task().run(&ctx);

        if auth_failure {
            assert!(task_skipped(&result.expect("auth failure should skip")).contains("GitHub"));
        } else {
            assert!(
                format!(
                    "{:#}",
                    result.expect_err("non-auth failure should propagate")
                )
                .contains(message)
            );
        }
    }
}

#[test]
fn auth_failure_detection_cases() {
    for (message, expected) in [
        ("HTTP 403 Forbidden", true),
        ("terminal prompts disabled", true),
        ("archive extraction failed", false),
    ] {
        assert_eq!(looks_like_auth_failure(message), expected, "{message}");
    }
}
