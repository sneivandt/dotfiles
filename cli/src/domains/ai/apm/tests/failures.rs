//! Failure classification, cancellation, and readiness tests for APM.

use super::*;

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
        let mut run = ApmRun::new(dir.path(), 1);
        run.command(if update { UPDATE } else { INSTALL }, move |_| {
            Err(command_failure(message))
        });
        let ctx = linux_context(dir.path(), run.mock);
        let result = if update {
            update_task()
        } else {
            install_task()
        }
        .run(&ctx);

        if auth_failure {
            assert!(matches!(
                result.expect("auth failure should leave work unmet"),
                TaskResult::Skipped {
                    reason,
                    kind: crate::engine::SkipKind::UnmetWork,
                } if reason.contains("GitHub")
            ));
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
        let mut run = ApmRun::new(dir.path(), 1);
        run.command(if update { UPDATE } else { INSTALL }, |_| {
            Err(ExecError::Cancelled {
                command: "apm".to_string(),
                result: ExecResult::failure(
                    "",
                    "authentication required before cancellation",
                    None,
                ),
            })
        });
        let ctx = linux_context(dir.path(), run.mock);
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
        let mut run = ApmRun::new(dir.path(), 0);
        run.ok(INSTALL, "installed\n");
        run.command(&["experimental", "enable", target], |_| {
            Err(ExecError::Cancelled {
                command: "apm".to_string(),
                result: ExecResult::failure("", "cancelled", None),
            })
        });
        let ctx = if target == "copilot-app" {
            linux_context(dir.path(), run.mock)
        } else {
            make_windows_cowork_context(dir.path(), run.mock)
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
        let task = install_task();
        let requirements = task.ordering_requirements();
        let state = requirements[0].check(&ctx, crate::engine::RequirementStage::Ordering);
        match case {
            "ready" => assert!(matches!(
                state,
                crate::engine::RequirementState::Satisfied(())
            )),
            "missing-apm" => assert!(matches!(
                state,
                crate::engine::RequirementState::Unsatisfied { .. }
            )),
            _ => assert!(matches!(
                state,
                crate::engine::RequirementState::Unknown { .. }
            )),
        }
    }
}

#[test]
fn missing_apm_blocks_ordering_and_update_preview_but_not_install_preview() {
    use crate::engine::{RequirementStage, RequirementState};

    let dir = tempfile::tempdir_in(".").unwrap();
    write_default_home_fragment(dir.path());
    let mut mock = MockExecutor::new();
    mock.expect_which()
        .times(2)
        .with(mockall::predicate::eq("apm"))
        .return_const(false);
    let ctx = linux_context(dir.path(), mock).with_dry_run(true);
    let install = install_task();
    assert!(matches!(
        install.ordering_requirements()[0].check(&ctx, RequirementStage::Ordering),
        RequirementState::Unsatisfied { .. },
    ));
    assert_task_changed(&install.run(&ctx).unwrap());
    assert!(task_skipped(&update_task().run(&ctx).unwrap()).contains("apm not found"));
    assert!(!dir.path().join(".apm/apm.yml").exists());
    assert!(!dir.path().join(".apm/apm.lock.yaml").exists());
}

#[test]
fn apm_respects_an_inactive_owned_fragment_left_in_the_home() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join(".apm/config");
    let retired = dir.path().join("retired.yml");
    let link = config.join("retired.yml");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(&retired, test_fixture::DEFAULT_FRAGMENT).unwrap();
    crate::infra::fs::create_native_symlink(&retired, &link, false).unwrap();
    let ctx = linux_context(dir.path(), MockExecutor::new());
    let task = InstallApmPackages::new(
        crate::infra::ConfigHandle::new(Vec::new()),
        ApmPackageMode::Install,
    )
    .with_inactive_fragments(crate::infra::ConfigHandle::new(InactiveApmFragments::new(
        vec![ApmFragmentSource::new(
            retired,
            ".apm/config/retired.yml".into(),
        )],
        Vec::new(),
    )));
    assert!(!task.should_run(&ctx));
    assert!(install_task().should_run(&ctx));
    assert!(link.symlink_metadata().unwrap().is_symlink());
}

#[test]
fn install_reports_lock_metadata_changes_without_inventing_dependency_changes() {
    let dir = tempfile::tempdir().unwrap();
    write_current_manifest_and_lock(dir.path());
    let mut run = ApmRun::new(dir.path(), 1);
    run.command(INSTALL, |home| {
        std::fs::write(
            home.join(".apm/apm.lock.yaml"),
            "dependencies: []\nmetadata: updated\n",
        )
        .unwrap();
        Ok(ExecResult::success(""))
    });
    let capture = Arc::new(CapturingOutput::default());
    let ctx = linux_context(dir.path(), run.mock).with_log(Arc::<CapturingOutput>::clone(&capture));
    let result = install_task().run(&ctx).unwrap();
    assert_task_changed(&result);
    assert_eq!(
        task_batch(&result).message(),
        Some("updated APM configuration")
    );
    let actions = capture.actions();
    assert_eq!(actions.len(), 1, "{actions:?}");
    assert_eq!(actions[0].subject, "APM lock state");
}
