use super::*;

#[test]
fn install_packages_run_reports_the_missing_native_manager() {
    for (os, package, manager) in [
        (Os::Linux, "git", "pacman"),
        (Os::Windows, "Git.Git", "winget"),
    ] {
        let packages = packages(&[(package, false)]);
        let ctx = make_platform_context_with_which(
            PathBuf::from("/repo"),
            None,
            os,
            os == Os::Linux,
            false,
        );
        let result = InstallPackages::new(packages).run(&ctx).unwrap();
        let reason = task_skipped(&result);
        assert_eq!(reason, format!("{manager} not found"), "{os:?}");
    }
}

#[test]
fn pacman_on_path_does_not_authorize_an_upgrade_on_non_arch_linux() {
    let task = InstallPackages::new(packages(&[("git", false)]));
    let ctx = make_package_context(
        PathBuf::from("fixture-repository"),
        None,
        Os::Linux,
        false,
        MockExecutor::new(),
    );
    assert_eq!(
        task_skipped(&task.run(&ctx).unwrap()),
        "native package installation requires Arch Linux or Windows"
    );
}

fn native_inventory(before: &str) -> ScriptedExecutor {
    ScriptedExecutor::new()
        .with_programs(&["pacman", "sudo"])
        .command(inventory(), Ok(ExecResult::success(before)))
}

#[test]
fn install_packages_batch_installs_missing_packages_on_arch() {
    let root = tempfile::tempdir_in(".").unwrap();
    let packages = packages(&[
        ("git", false),
        ("vim", false),
        ("aur-package-must-not-be-installed", true),
    ]);
    let mock = native_inventory("vim 9.0\n")
        .command(
            native_install(PackageManager::Pacman, &["git"]),
            Ok(ExecResult::success("")),
        )
        .command(inventory(), Ok(ExecResult::success("vim 9.0\ngit 2.0\n")));
    let ctx = make_package_context(root.path().to_path_buf(), None, Os::Linux, true, mock);
    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    let stats = task_batch(&result);
    assert!(
        stats.changed_count() == 1 && stats.already_ok_count() == 1 && stats.failed_count() == 0,
        "expected changed package task result after batch install, got {result:?}"
    );
}

#[test]
fn install_packages_all_already_installed_returns_ok() {
    let root = tempfile::tempdir_in(".").unwrap();
    let packages = packages(&[("git", false)]);
    let mock = native_inventory("git 2.40\n");
    let ctx = make_package_context(root.path().to_path_buf(), None, Os::Linux, true, mock);
    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    assert_task_ok(&result);
}

#[test]
fn install_packages_dry_run_reports_missing_packages() {
    let root = tempfile::tempdir_in(".").unwrap();
    let packages = packages(&[("git", false), ("vim", false)]);
    let mock = native_inventory("");
    let output = Arc::new(CapturingOutput::default());
    let ctx = make_package_context(root.path().to_path_buf(), None, Os::Linux, true, mock)
        .with_log(Arc::<CapturingOutput>::clone(&output))
        .with_dry_run(true);
    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    let warnings = output.messages_of(crate::infra::logging::MsgKind::Warn);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("may upgrade other installed packages"));
    let stats = task_batch(&result);
    assert_eq!(
        (
            stats.changed_count(),
            stats.already_ok_count(),
            stats.failed_count()
        ),
        (2, 0, 0)
    );
}

#[test]
fn native_inventory_failure_prevents_planning_and_installation() {
    for dry_run in [false, true] {
        let fixture = tempfile::tempdir_in(".").unwrap();

        let packages = packages(&[("git", false)]);
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .once()
            .withf(|program| program == "pacman")
            .return_const(true);
        mock.expect_execute()
            .once()
            .withf(is_native_inventory)
            .returning(|_| {
                Err(ExecError::non_zero(
                    "pacman -Q",
                    ExecResult::failure(
                        "partial-package 1.0\n",
                        "fixture corrupt database",
                        Some(42),
                    ),
                ))
            });
        let ctx = make_package_context(fixture.path().to_path_buf(), None, Os::Linux, true, mock)
            .with_dry_run(dry_run);
        let error = InstallPackages::new(packages).run(&ctx).unwrap_err();
        let details = format!("{error:#}");
        assert!(details.contains("fixture corrupt database"), "{details}");
        assert!(details.contains("42"), "{details}");
    }
}

#[test]
fn install_packages_returns_failed_when_batch_install_fails() {
    let root = tempfile::tempdir_in(".").unwrap();
    let packages = packages(&[("git", false)]);
    let error = ExecError::non_zero(
        "pacman",
        ExecResult::failure("", "database locked", Some(1)),
    );
    let mock = native_inventory("")
        .command(native_install(PackageManager::Pacman, &["git"]), Err(error))
        .command(inventory(), Ok(ExecResult::success("")));
    let ctx = make_package_context(root.path().to_path_buf(), None, Os::Linux, true, mock);
    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    let stats = task_batch(&result);
    assert_eq!(
        (
            stats.changed_count(),
            stats.already_ok_count(),
            stats.failed_count()
        ),
        (0, 0, 1)
    );
}

#[test]
fn partial_package_batch_preserves_changed_and_already_installed_counts() {
    for all_installed in [false, true] {
        let packages = packages(&[("first", false), ("second", false), ("existing", false)]);
        let error = ExecError::non_zero(
            "pacman",
            ExecResult::failure("", "fixture second package failed", Some(1)),
        );
        let after = if all_installed {
            "existing 1.0\nfirst 1.0\nsecond 1.0\n"
        } else {
            "existing 1.0\nfirst 1.0\n"
        };
        let mock = native_inventory("existing 1.0\n")
            .command(
                native_install(PackageManager::Pacman, &["first", "second"]),
                Err(error),
            )
            .command(inventory(), Ok(ExecResult::success(after)));
        let ctx = make_package_context(
            PathBuf::from("fixture-repository"),
            None,
            Os::Linux,
            true,
            mock,
        );

        let result = InstallPackages::new(packages).run(&ctx);
        let stats = if all_installed {
            let report = result
                .as_ref()
                .unwrap_err()
                .downcast_ref::<BatchReport>()
                .unwrap();
            assert_eq!(report.completion(), BatchCompletion::Failed);
            assert_eq!(report.interrupted_count(), 0);
            assert_eq!(report.not_attempted_count(), 0);
            report.stats()
        } else {
            task_batch(result.as_ref().unwrap())
        };

        assert_eq!(
            (
                stats.changed_count(),
                stats.already_ok_count(),
                stats.failed_count()
            ),
            if all_installed { (2, 1, 0) } else { (1, 1, 1) }
        );
    }
}

#[test]
fn native_batch_timeout_preserves_observed_progress_and_retries_only_missing_packages() {
    let root = tempfile::tempdir_in(".").unwrap();
    let packages = packages(&[("first", true), ("second", true), ("third", true)]);
    let mut mock = MockExecutor::new();
    let mut inventory = "";
    mock.expect_execute().times(6).returning(move |spec| {
        if is_native_inventory(&spec) {
            return Ok(ExecResult::success(inventory));
        }
        assert_eq!(spec.program(), "/usr/bin/paru");
        assert!(spec.is_checked());
        if inventory.is_empty() {
            assert_eq!(
                spec.arguments(),
                ["-S", "--needed", "--noconfirm", "first", "second", "third"]
            );
            inventory = "first 1\nthird 1\n";
            Err(ExecError::TimedOut {
                command: "fixture batch".into(),
                timeout: std::time::Duration::from_secs(30),
                result: ExecResult::failure("", "fixture timeout", None),
            })
        } else {
            assert_eq!(
                spec.arguments(),
                ["-S", "--needed", "--noconfirm", "second"]
            );
            inventory = "first 1\nsecond 1\nthird 1\n";
            Ok(ExecResult::success(""))
        }
    });
    let ctx = make_package_context(root.path().into(), None, Os::Linux, true, mock);
    let operation = PackageInstallOperation::new(packages.get().clone(), PackageManager::Paru);
    let error = process_operation(&ctx, &operation).unwrap_err();
    assert!(matches!(
        error.downcast_ref::<ExecError>(),
        Some(ExecError::TimedOut { .. })
    ));
    let report = error.downcast_ref::<BatchReport>().unwrap();
    assert_eq!(report.completion(), BatchCompletion::Failed);
    assert_eq!(report.stats().changed_count(), 2);
    assert_eq!(report.stats().failed_count(), 1);
    assert!(format!("{error:#}").contains("second"));

    let retry = process_operation(&ctx, &operation).unwrap();
    let stats = task_batch(&retry);
    assert_eq!(stats.changed_count(), 1);
    assert_eq!(stats.already_ok_count(), 2);
    assert_eq!(stats.failed_count(), 0);
}

#[test]
fn failed_post_install_inventory_does_not_guess_item_outcomes() {
    use crate::engine::{TaskAssessment, TaskOutcome, task::execute_assessed};
    use crate::infra::logging::TaskStatus;

    for batch_succeeded in [true, false] {
        let root = tempfile::tempdir_in(".").unwrap();
        let log = Arc::new(CapturingOutput::default());
        let mut inventory = "existing 1\n";
        let mut fail_next_query = false;
        let mut mock = MockExecutor::new();
        mock.expect_execute().times(10).returning(move |spec| {
            if spec.program() == "pacman" && spec.arguments() == ["-Q", "paru"] {
                assert!(!spec.is_checked());
                return Ok(ExecResult::success("paru 1"));
            }
            if is_native_inventory(&spec) {
                if std::mem::take(&mut fail_next_query) {
                    return Err(ExecError::non_zero(
                        "pacman -Q",
                        ExecResult::failure(inventory, "fixture inventory error", Some(7)),
                    ));
                }
                return Ok(ExecResult::success(inventory));
            }
            assert_eq!(spec.program(), "/usr/bin/paru");
            assert!(spec.is_checked());
            if spec.arguments() == ["--version"] {
                return Ok(ExecResult::success("paru fixture"));
            }
            if inventory == "existing 1\n" {
                assert_eq!(
                    spec.arguments(),
                    ["-S", "--needed", "--noconfirm", "first", "second"]
                );
                inventory = "existing 1\nfirst 1\n";
                fail_next_query = true;
                if !batch_succeeded {
                    return Err(ExecError::non_zero(
                        "paru",
                        ExecResult::failure("", "fixture transaction error", Some(42)),
                    ));
                }
            } else {
                assert_eq!(
                    spec.arguments(),
                    ["-S", "--needed", "--noconfirm", "second"]
                );
                inventory = "existing 1\nfirst 1\nsecond 1\n";
            }
            Ok(ExecResult::success(""))
        });
        let task = InstallAurPackages::new(packages(&[
            ("existing", true),
            ("first", true),
            ("second", true),
        ]));
        let ctx = make_package_context(root.path().into(), None, Os::Linux, true, mock)
            .with_log(Arc::<CapturingOutput>::clone(&log));
        let assessment = TaskAssessment::applicable();
        let failed = execute_assessed(&task, &assessment, &ctx);
        let retry = execute_assessed(&task, &assessment, &ctx);

        assert_eq!(failed.outcome, TaskOutcome::Failed);
        assert_eq!(retry.outcome, TaskOutcome::Satisfied);
        let records = log.records();
        let [first, second] = records.as_slice() else {
            panic!("unexpected task records: {records:?}")
        };
        assert_eq!(first.status, TaskStatus::Failed);
        assert_eq!(
            (first.actions.applied, first.actions.failed),
            (0, 0),
            "batch_succeeded={batch_succeeded}"
        );
        let message = first.message.as_deref().unwrap();
        assert!(
            message.contains("1 already ok") && message.contains("fixture inventory error"),
            "{message}"
        );
        if !batch_succeeded {
            assert!(message.contains("fixture transaction error"), "{message}");
        }
        assert_eq!((second.actions.applied, second.actions.failed), (1, 0));
    }
}

#[test]
fn failed_timeout_recheck_preserves_the_timeout_or_new_cancellation() {
    for cancelled in [false, true] {
        let root = tempfile::tempdir_in(".").unwrap();
        let mut mock = MockExecutor::new();
        let mut calls = 0;
        mock.expect_execute().times(3).returning(move |spec| {
            calls += 1;
            match calls {
                1 => {
                    assert!(is_native_inventory(&spec));
                    Ok(ExecResult::success(""))
                }
                2 => {
                    assert_eq!(spec.program(), "/usr/bin/paru");
                    Err(ExecError::TimedOut {
                        command: "fixture batch".into(),
                        timeout: std::time::Duration::from_secs(30),
                        result: ExecResult::failure("", "fixture timeout", None),
                    })
                }
                _ => {
                    assert!(is_native_inventory(&spec));
                    let result = ExecResult::failure("", "fixture recheck failure", None);
                    if cancelled {
                        Err(ExecError::Cancelled {
                            command: "pacman -Q".into(),
                            result,
                        })
                    } else {
                        Err(ExecError::non_zero("pacman -Q", result))
                    }
                }
            }
        });
        let ctx = make_package_context(root.path().into(), None, Os::Linux, true, mock);
        let operation = PackageInstallOperation::new(
            packages(&[("missing", true)]).get().clone(),
            PackageManager::Paru,
        );
        let error = process_operation(&ctx, &operation).unwrap_err();
        let detail = format!("{error:#}");
        assert!(detail.contains("fixture timeout") && detail.contains("fixture recheck failure"));
        let report = error.downcast_ref::<BatchReport>().unwrap();
        let cause = error.downcast_ref::<ExecError>().unwrap();
        if cancelled {
            assert!(cause.is_cancelled());
            assert_eq!(report.completion(), BatchCompletion::Interrupted);
        } else {
            assert!(matches!(cause, ExecError::TimedOut { .. }));
            assert_eq!(report.completion(), BatchCompletion::Failed);
        }
        assert_eq!(report.stats().changed_count(), 0);
    }
}

#[test]
fn install_packages_propagates_batch_cancellation() {
    let root = tempfile::tempdir_in(".").unwrap();
    let packages = packages(&[("git", false)]);
    let mock = native_inventory("").command(
        native_install(PackageManager::Pacman, &["git"]),
        Err(ExecError::Cancelled {
            command: "sudo pacman".to_string(),
            result: ExecResult::failure("", "", None),
        }),
    );
    let ctx = make_package_context(root.path().to_path_buf(), None, Os::Linux, true, mock);

    let err = InstallPackages::new(packages)
        .run(&ctx)
        .expect_err("package cancellation must escape task accounting");

    assert!(
        err.downcast_ref::<ExecError>()
            .is_some_and(ExecError::is_cancelled)
    );
}
