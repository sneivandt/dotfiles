//! Focused tests for native APM install/update ownership.

use std::path::Path;
use std::sync::Arc;

use super::test_fixture::{
    ApmRun, INSTALL, PREVIEW_UPDATE, UPDATE, expect_which_apm, install_task,
    make_context_with_home, make_windows_cowork_context, update_task, write_copilot_app_db,
    write_current_manifest_and_lock, write_default_home_fragment,
};
use super::*;
use crate::engine::Context;
use crate::infra::exec::{ExecError, ExecResult, MockExecutor};
use crate::infra::logging::{self, Log, Logger};
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::{
    CapturingOutput, assert_task_changed, assert_task_ok, task_batch, task_skipped,
};

fn command_failure(message: &str) -> ExecError {
    ExecError::non_zero("apm", ExecResult::failure("", message, Some(1)))
}

fn linux_context(home: &Path, executor: MockExecutor) -> Context {
    make_context_with_home(home, Platform::new(Os::Linux, false), executor)
}

#[test]
fn agent_package_update_never_invokes_apm_to_replace_a_personal_manifest() {
    let dir = tempfile::tempdir().unwrap();
    write_default_home_fragment(dir.path());
    let manifest = dir.path().join(".apm/apm.yml");
    let contents = b"name: my-existing-packages\n";
    std::fs::write(&manifest, contents).unwrap();
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    let ctx = linux_context(dir.path(), mock);
    let error = update_task().run(&ctx).unwrap_err();
    assert!(
        error.to_string().contains("unmanaged APM manifest"),
        "{error:#}"
    );
    assert_eq!(std::fs::read(manifest).unwrap(), contents);
}

#[test]
fn apm_apply_refuses_a_shared_home_lock_before_invoking_apm() {
    let dir = tempfile::tempdir().unwrap();
    write_default_home_fragment(dir.path());
    let path = dir.path().join(".apm/.dotfiles-run.lock");
    let lock = crate::infra::run_lock::RunLock::acquire_path(&path, "first checkout").unwrap();
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    let ctx = linux_context(dir.path(), mock);
    let error = update_task().run(&ctx).unwrap_err();
    assert!(
        error.to_string().contains("another dotfiles run holds"),
        "{error:#}"
    );
    assert!(!dir.path().join(".apm/apm.yml").exists());
    drop(lock);
    assert!(crate::infra::run_lock::RunLock::acquire_path(&path, "retry").is_ok());
}

#[test]
fn cowork_repairs_report_changes_even_when_the_lock_is_unchanged() {
    for update in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        write_current_manifest_and_lock(dir.path());
        let lock = "dependencies:\n  - deployed_files:\n      - .agents/skills/example/SKILL.md\n    target_subset: [agent-skills, copilot-cowork]\n";
        std::fs::write(dir.path().join(".apm/apm.lock.yaml"), lock).unwrap();
        let source = dir.path().join(".agents/skills/example");
        let target = dir
            .path()
            .join("OneDrive - Test/Documents/Cowork/skills/example");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(source.join("SKILL.md"), "stale fixture").unwrap();
        std::fs::write(target.join("cowork-owned.txt"), "preserve").unwrap();
        let mut run = ApmRun::new(dir.path(), 2);
        for _ in 0..2 {
            if update {
                run.ok(UPDATE, "");
            }
            run.ok(INSTALL, "");
            run.ok(&["experimental", "enable", "copilot-cowork"], "");
        }
        let ctx = make_windows_cowork_context(dir.path(), run.mock);
        assert!(cowork::reconcile_cowork_skills(&ctx).unwrap());
        std::fs::write(source.join("SKILL.md"), "current fixture").unwrap();
        let task = if update {
            update_task()
        } else {
            install_task()
        };
        let result = task.run(&ctx).unwrap();
        assert_task_changed(&result);
        let message = if update {
            "updated APM deployments"
        } else {
            "updated APM configuration"
        };
        assert_eq!(task_batch(&result).message(), Some(message));
        assert_task_ok(&task.run(&ctx).unwrap());
        assert_eq!(
            std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "current fixture"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("cowork-owned.txt")).unwrap(),
            "preserve"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".apm/apm.lock.yaml")).unwrap(),
            lock
        );
    }
}

const REFS_CURRENT: &str = "All dependencies already at their latest matching refs.\n";

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
    let mut run = ApmRun::new(dir.path(), 1);
    run.ok(INSTALL, "installed\n");
    let ctx = linux_context(dir.path(), run.mock);

    let result = install_task().run(&ctx).expect("run install");

    assert_task_ok(&result);
}

#[test]
fn install_reports_manifest_and_lock_changes() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());
    let mut run = ApmRun::new(dir.path(), 1);
    run.command(INSTALL, |home| {
        std::fs::write(
            home.join(".apm/apm.lock.yaml"),
            "dependencies:\n- repo_url: example/plugin\n",
        )
        .expect("write lock");
        Ok(ExecResult::success("installed\n"))
    });
    let ctx = linux_context(dir.path(), run.mock);

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
    let mut run = ApmRun::new(dir.path(), 1);
    run.copilot_install();
    let ctx = linux_context(dir.path(), run.mock);

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
    let mut run = ApmRun::new(dir.path(), 1);
    run.ok(INSTALL, "installed\n");
    run.workflow_install();
    let ctx = linux_context(dir.path(), run.mock);

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

    let mut run = ApmRun::new(dir.path(), 1);
    run.command(INSTALL, |home| {
        std::fs::write(
            home.join(".apm/apm.lock.yaml"),
            "dependencies:\n  - deployed_files:\n      - \
                 .agents/skills/example/SKILL.md\n    target_subset: [agent-skills, \
                 copilot-cowork]\n",
        )
        .expect("write lock");
        Ok(ExecResult::success("installed\n"))
    });
    run.enable("copilot-cowork");
    let ctx = make_windows_cowork_context(dir.path(), run.mock);

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
    let mut run = ApmRun::new(dir.path(), 1);
    run.command(INSTALL, |home| {
        let lock =
            std::fs::read_to_string(home.join(".apm/apm.lock.yaml")).expect("read sanitized lock");
        assert!(!lock.contains("cowork://"));
        Ok(ExecResult::success("installed\n"))
    });
    let ctx = linux_context(dir.path(), run.mock);

    assert_task_changed(&install_task().run(&ctx).expect("run install"));
}

#[test]
fn cowork_dry_run_preserves_legacy_lock_skills_and_ownership() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_and_lock(dir.path());
    let lock = "dependencies:\n  - deployed_files:\n      - cowork://skills/example/SKILL.md\n";
    let lock_path = dir.path().join(".apm").join("apm.lock.yaml");
    std::fs::write(&lock_path, lock).unwrap();
    let target = dir
        .path()
        .join("OneDrive - Test")
        .join("Documents")
        .join("Cowork")
        .join("skills");
    std::fs::create_dir_all(target.join("example")).unwrap();
    std::fs::write(target.join("example").join("SKILL.md"), "legacy").unwrap();
    let ctx = make_windows_cowork_context(dir.path(), MockExecutor::new()).with_dry_run(true);

    assert_task_changed(&install_task().run(&ctx).expect("preview install"));

    assert_eq!(std::fs::read_to_string(lock_path).unwrap(), lock);
    assert_eq!(
        std::fs::read_to_string(target.join("example").join("SKILL.md")).unwrap(),
        "legacy"
    );
    assert!(!target.join(".dotfiles-apm-skills.json").exists());
}

mod failures;
mod update;
