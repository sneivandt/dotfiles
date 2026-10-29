//! Focused tests for native APM install/update ownership.

use std::path::Path;
use std::sync::Arc;

use super::test_fixture::{
    assert_apm_command, expect_apm_install_without_enable, expect_copilot_app_enable,
    expect_copilot_app_workflow_install, expect_cowork_enable, expect_which_apm, has_env,
    install_task, make_context_with_home, make_windows_cowork_context, update_task,
    write_copilot_app_db, write_current_manifest_and_lock, write_default_home_fragment,
};
use super::*;
use crate::engine::Context;
use crate::infra::exec::{ExecError, ExecResult, MockExecutor};
use crate::infra::logging::{self, Log, Logger};
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::{assert_task_changed, assert_task_ok, task_batch, task_skipped};

fn command_failure(message: &str) -> ExecError {
    ExecError::non_zero("apm", ExecResult::failure("", message, Some(1)))
}

fn linux_context(home: &Path, executor: MockExecutor) -> Context {
    make_context_with_home(home, Platform::new(Os::Linux, false), executor)
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
        std::fs::write(source.join("SKILL.md"), "current fixture").unwrap();
        std::fs::write(target.join("SKILL.md"), "stale fixture").unwrap();
        std::fs::write(target.join("cowork-owned.txt"), "preserve").unwrap();
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .with(mockall::predicate::eq("apm"))
            .times(2)
            .return_const(true);
        mock.expect_execute()
            .times(2)
            .withf(move |spec| {
                spec.program() == "apm"
                    && spec.arguments()
                        == if update {
                            vec!["update", "-g", "--yes"]
                        } else {
                            vec!["install", "-g"]
                        }
            })
            .returning(|_| Ok(ExecResult::success("")));
        if update {
            mock.expect_execute()
                .times(2)
                .withf(|spec| spec.program() == "apm" && spec.arguments() == ["install", "-g"])
                .returning(|_| Ok(ExecResult::success("")));
        }
        mock.expect_execute()
            .times(2)
            .withf(|spec| {
                spec.program() == "apm"
                    && spec.arguments() == ["experimental", "enable", "copilot-cowork"]
            })
            .returning(|_| Ok(ExecResult::success("")));
        let ctx = make_windows_cowork_context(dir.path(), mock);
        let task: Box<dyn Task> = if update {
            Box::new(update_task())
        } else {
            Box::new(install_task())
        };
        assert_task_changed(&task.run(&ctx).unwrap());
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

fn expect_primary_install(mock: &mut MockExecutor, seq: &mut mockall::Sequence, home: &Path) {
    let home = home.to_path_buf();
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(move |spec| {
            assert_apm_command(&spec);
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
            assert_apm_command(&spec);
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

#[test]
fn update_then_install_reports_exact_lock_changes() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_and_lock(dir.path());
    std::fs::write(
        dir.path().join(".apm").join("apm.lock.yaml"),
        "dependencies:\n- repo_url: example/plugin\n  name: example-plugin\n  \
         resolved_commit: 1111111aaaa\n",
    )
    .expect("seed lock");
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
                "dependencies:\n- repo_url: example/plugin\n  name: example-plugin\n  \
                 resolved_commit: 2222222bbbb\n",
            )
            .expect("advance lock");
            Ok(ExecResult::success("updated\n"))
        });
    expect_primary_install(&mut mock, &mut seq, dir.path());
    let (log, _tmp, _guard) = logging::isolated_logger();
    let log = Arc::new(log);
    let log_output: Arc<dyn Log> = Arc::<Logger>::clone(&log);
    let ctx = linux_context(dir.path(), mock).with_log(log_output);

    let result = update_task().run(&ctx).expect("run update");

    assert_task_changed(&result);
    assert_eq!(
        task_batch(&result).message(),
        Some("updated 1 agent package")
    );
    let contents = std::fs::read_to_string(log.log_path().expect("log path")).expect("read log");
    assert!(
        contents.lines().any(|line| {
            let Some(logging::records::StoredRecord {
                record:
                    logging::records::Record::Action {
                        verb,
                        subject,
                        planned,
                        message,
                        ..
                    },
                ..
            }) = logging::records::StoredRecord::from_line(line)
            else {
                return false;
            };
            verb == "update"
                && !planned
                && subject == "example-plugin · commit 1111111 -> 2222222"
                && message == "update example-plugin · commit 1111111 -> 2222222"
        }),
        "agent package change should be emitted as task detail: {contents}"
    );
}

#[test]
fn update_writes_generated_manifest_then_updates_and_installs() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    let mut seq = mockall::Sequence::new();
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|spec| {
            assert_eq!(spec.arguments(), ["update", "-g", "--yes"]);
            let manifest = std::fs::read_to_string(
                spec.working_dir()
                    .expect("home")
                    .join(".apm")
                    .join("apm.yml"),
            )
            .expect("read generated manifest before update");
            assert!(manifest.contains("example/plugin"));
            Ok(ExecResult::success("updated\n"))
        });
    expect_primary_install(&mut mock, &mut seq, dir.path());
    let ctx = linux_context(dir.path(), mock);

    assert_task_changed(&update_task().run(&ctx).expect("run update"));
}

#[test]
fn update_reconciles_renamed_local_skills_when_remote_refs_are_current() {
    let dir = tempfile::tempdir().unwrap();
    write_current_manifest_and_lock(dir.path());
    let source = dir
        .path()
        .join("plugin/.apm/skills/agent-customization/SKILL.md");
    let old = dir.path().join(".agents/skills/apm-dotfiles/SKILL.md");
    let deployed = dir
        .path()
        .join(".agents/skills/agent-customization/SKILL.md");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(&source, "renamed local skill").unwrap();
    std::fs::write(&old, "old local skill").unwrap();
    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .times(2)
        .return_const(true);
    for _ in 0..2 {
        expect_primary_update(&mut mock, &mut seq, dir.path());
        let source = source.clone();
        let old = old.clone();
        let deployed = deployed.clone();
        mock.expect_execute().once().in_sequence(&mut seq).returning(move |spec| {
            assert_apm_command(&spec);
            assert_eq!(spec.arguments(), ["install", "-g"]);
            let home = spec.working_dir().unwrap();
            assert_eq!(source.strip_prefix(home).unwrap(), Path::new("plugin/.apm/skills/agent-customization/SKILL.md"));
            // Model native install reconciling local content and stale ownership.
            std::fs::create_dir_all(deployed.parent().unwrap()).unwrap();
            std::fs::copy(&source, &deployed).unwrap();
            if old.exists() {
                std::fs::remove_file(&old).unwrap();
            }
            std::fs::write(home.join(".apm/apm.lock.yaml"),
                "dependencies:\n- name: dot-skill\n  deployed_files: [.agents/skills/agent-customization/SKILL.md]\n").unwrap();
            Ok(ExecResult::success("installed\n"))
        });
    }
    let ctx = linux_context(dir.path(), mock);
    assert_task_changed(&update_task().run(&ctx).unwrap());
    assert!(!old.exists());
    assert_eq!(
        std::fs::read(&deployed).unwrap(),
        std::fs::read(&source).unwrap()
    );
    assert_task_ok(&update_task().run(&ctx).unwrap());
}

#[test]
fn update_does_not_report_current_when_followup_install_fails() {
    for message in ["archive extraction failed", "fatal: Authentication failed"] {
        let dir = tempfile::tempdir().unwrap();
        write_current_manifest_and_lock(dir.path());
        let mut mock = MockExecutor::new();
        let mut seq = mockall::Sequence::new();
        expect_which_apm(&mut mock, true);
        expect_primary_update(&mut mock, &mut seq, dir.path());
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .returning(move |spec| {
                assert_apm_command(&spec);
                assert_eq!(spec.arguments(), ["install", "-g"]);
                Err(command_failure(message))
            });
        let ctx = linux_context(dir.path(), mock);
        let result = update_task().run(&ctx);
        if message.contains("Authentication") {
            assert!(task_skipped(&result.unwrap()).contains("GitHub"));
        } else {
            assert!(format!("{:#}", result.unwrap_err()).contains(message));
        }
    }
}

#[test]
fn update_reports_ok_when_lock_is_byte_identical() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_and_lock(dir.path());
    let mut mock = MockExecutor::new();
    let mut seq = mockall::Sequence::new();
    expect_which_apm(&mut mock, true);
    expect_primary_update(&mut mock, &mut seq, dir.path());
    expect_primary_install(&mut mock, &mut seq, dir.path());
    let ctx = linux_context(dir.path(), mock);

    assert_task_ok(&update_task().run(&ctx).expect("run update"));
}

#[test]
fn update_dry_run_uses_native_apm_plan() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_and_lock(dir.path());
    let manifest_path = dir.path().join(".apm/apm.yml");
    let lock_path = dir.path().join(".apm/apm.lock.yaml");
    let manifest_before = std::fs::read(&manifest_path).unwrap();
    let lock_before = std::fs::read(&lock_path).unwrap();
    let expected_home = dir.path().to_path_buf();
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    mock.expect_execute().once().returning(move |spec| {
        assert_apm_command(&spec);
        assert_eq!(spec.working_dir(), Some(expected_home.as_path()));
        assert_eq!(spec.arguments(), ["update", "-g", "--dry-run"]);
        Ok(ExecResult::success("would update example/plugin\n"))
    });
    let ctx = linux_context(dir.path(), mock).with_dry_run(true);

    assert!(matches!(
        update_task().run(&ctx).expect("preview update"),
        TaskResult::DryRun
    ));
    assert_eq!(std::fs::read(manifest_path).unwrap(), manifest_before);
    assert_eq!(std::fs::read(lock_path).unwrap(), lock_before);
}

#[test]
fn update_dry_run_does_not_plan_against_a_stale_manifest() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());
    let mut mock = MockExecutor::new();
    expect_which_apm(&mut mock, true);
    let ctx = linux_context(dir.path(), mock).with_dry_run(true);

    assert_task_changed(&update_task().run(&ctx).expect("preview update"));
    assert!(!dir.path().join(".apm").join("apm.yml").exists());
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
fn invalid_target_configuration_fails_before_manifest_writes_or_native_commands() {
    for update in [false, true] {
        for dry_run in [false, true] {
            let dir = tempfile::tempdir_in(".").unwrap();
            write_default_home_fragment(dir.path());
            let config = dir.path().join(".apm").join("config.json");
            std::fs::write(&config, "{invalid").unwrap();
            let mut mock = MockExecutor::new();
            if update || !dry_run {
                expect_which_apm(&mut mock, true);
            }
            let ctx = linux_context(dir.path(), mock).with_dry_run(dry_run);
            let error = if update {
                update_task()
            } else {
                install_task()
            }
            .run(&ctx)
            .unwrap_err();

            assert!(format!("{error:#}").contains("config.json"), "{error:#}");
            assert_eq!(std::fs::read_to_string(&config).unwrap(), "{invalid");
            assert!(!dir.path().join(".apm").join("apm.yml").exists());
            assert!(!dir.path().join(".apm").join("apm.lock.yaml").exists());
        }
    }
}

#[test]
fn native_commands_classify_auth_failures_but_propagate_other_failures() {
    for (update, message, auth_failure) in [
        (
            false,
            "fatal: Authentication failed; terminal prompts disabled",
            true,
        ),
        (
            true,
            "fatal: Authentication failed; terminal prompts disabled",
            true,
        ),
        (false, "archive extraction failed", false),
        (true, "archive extraction failed", false),
    ] {
        let dir = tempfile::tempdir().expect("create temp dir");
        write_default_home_fragment(dir.path());
        let mut mock = MockExecutor::new();
        expect_which_apm(&mut mock, true);
        let expected_home = dir.path().to_path_buf();
        mock.expect_execute().once().returning(move |spec| {
            assert_apm_command(&spec);
            assert_eq!(spec.working_dir(), Some(expected_home.as_path()));
            assert_eq!(
                spec.arguments(),
                if update {
                    vec!["update", "-g", "--yes"]
                } else {
                    vec!["install", "-g"]
                }
            );
            Err(command_failure(message))
        });
        let ctx = linux_context(dir.path(), mock);
        let result = if update {
            update_task()
        } else {
            install_task()
        }
        .run(&ctx);

        if auth_failure {
            assert!(task_skipped(&result.expect("auth failure should skip")).contains("GitHub"));
        } else {
            let error = result.expect_err("non-auth failure should propagate");
            assert_eq!(
                error.to_string(),
                if update {
                    "updating agent packages"
                } else {
                    "running apm install"
                },
                "command failure should identify the operation"
            );
            assert!(format!("{error:#}").contains(message));
        }
    }
}

#[test]
fn cancelled_native_commands_preserve_cancellation_instead_of_skipping() {
    for update in [false, true] {
        let dir = tempfile::tempdir_in(".").unwrap();
        write_current_manifest_and_lock(dir.path());
        let mut mock = MockExecutor::new();
        expect_which_apm(&mut mock, true);
        let expected_home = dir.path().to_path_buf();
        mock.expect_execute().once().returning(move |spec| {
            assert_apm_command(&spec);
            assert_eq!(spec.working_dir(), Some(expected_home.as_path()));
            assert_eq!(
                spec.arguments(),
                if update {
                    vec!["update", "-g", "--yes"]
                } else {
                    vec!["install", "-g"]
                }
            );
            Err(ExecError::Cancelled {
                command: "apm".to_string(),
                result: ExecResult::failure(
                    "",
                    "authentication required before cancellation",
                    None,
                ),
            })
        });
        let ctx = linux_context(dir.path(), mock);
        let error = if update {
            update_task()
        } else {
            install_task()
        }
        .run(&ctx)
        .unwrap_err();
        assert!(
            error
                .downcast_ref::<ExecError>()
                .is_some_and(ExecError::is_cancelled),
            "{error:#}"
        );
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

#[test]
fn cancelled_target_enablement_stops_before_native_install_or_cowork_copy() {
    for target in ["copilot-app", "copilot-cowork"] {
        let dir = tempfile::tempdir_in(".").unwrap();
        write_current_manifest_and_lock(dir.path());
        if target == "copilot-app" {
            write_copilot_app_db(dir.path());
        }
        let mut mock = MockExecutor::new();
        let mut sequence = mockall::Sequence::new();
        expect_primary_install(&mut mock, &mut sequence, dir.path());
        mock.expect_execute()
            .once()
            .in_sequence(&mut sequence)
            .returning(move |spec| {
                assert_apm_command(&spec);
                assert_eq!(spec.arguments(), ["experimental", "enable", target]);
                Err(ExecError::Cancelled {
                    command: "apm".to_string(),
                    result: ExecResult::failure("", "cancelled", None),
                })
            });
        let ctx = if target == "copilot-app" {
            linux_context(dir.path(), mock)
        } else {
            make_windows_cowork_context(dir.path(), mock)
        };
        let error = managed_targets::ManagedTargets::detect(&ctx)
            .unwrap()
            .run_apm_command(&ctx, commands::ApmCommand::Install)
            .unwrap_err();

        assert!(
            error
                .downcast_ref::<ExecError>()
                .is_some_and(ExecError::is_cancelled),
            "{target}: {error:#}"
        );
        assert!(
            !dir.path().join("OneDrive - Test").exists(),
            "cancelled target enablement must not copy Cowork files"
        );
    }
}

#[test]
fn early_apm_readiness_retains_waits_without_apm_or_with_uncertain_targets() {
    for case in ["missing-apm", "ready"]
        .into_iter()
        .chain(cfg!(unix).then_some("unreadable-target"))
    {
        let dir = tempfile::tempdir().unwrap();
        let mut mock = MockExecutor::new();
        expect_which_apm(&mut mock, case != "missing-apm");
        if case == "unreadable-target" {
            // A file where the target's parent directory should be forces a
            // deterministic lookup error without relying on user permissions.
            std::fs::write(dir.path().join(".copilot"), "not a directory").unwrap();
        }
        let ctx = linux_context(dir.path(), mock);
        assert_eq!(
            install_task().prerequisites_available(&ctx),
            case == "ready",
            "{case}"
        );
    }
}
