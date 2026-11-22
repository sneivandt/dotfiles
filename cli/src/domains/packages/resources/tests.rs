//! Tests for package resources and providers.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;

use super::package::*;
use super::report::PackageInstallReport;
use super::winget::{mock_inventory, parse_winget_ids};
use crate::domains::packages::test_fixture::{
    inventory, native_install, pacman_uses_sudo, winget_install,
};
use crate::engine::{Resource, ResourceChange, ResourceState};
use crate::infra::exec::{ExecError, ExecResult, Executor, MockExecutor};

fn expect_sudo_lookup_if_needed(mock: &mut MockExecutor) {
    if pacman_uses_sudo() {
        mock.expect_which()
            .once()
            .with(mockall::predicate::eq("sudo"))
            .returning(|_| true);
    }
}

fn install_batch(
    manager: PackageManager,
    names: &[&str],
    mock: MockExecutor,
    progress: &dyn Fn(&str),
) -> Result<PackageInstallReport> {
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let resources: Vec<_> = names
        .iter()
        .map(|&name| PackageResource::new(name.into(), manager, Arc::clone(&executor)))
        .collect();
    install_missing_packages(
        manager,
        &resources.iter().collect::<Vec<_>>(),
        &*executor,
        progress,
    )
}

/// A started transaction is reconciled against exactly one fresh inventory.
/// A spawn failure need not (and must not) inspect the package database.
fn native_batch(
    manager: PackageManager,
    names: &[&str],
    transaction: Result<ExecResult, ExecError>,
    verification: Option<Result<ExecResult, ExecError>>,
) -> Result<PackageInstallReport> {
    let mut mock = MockExecutor::new();
    if manager == PackageManager::Pacman {
        expect_sudo_lookup_if_needed(&mut mock);
    }
    let mut sequence = mockall::Sequence::new();
    let steps = std::iter::once((native_install(manager, names), transaction))
        .chain(verification.map(|result| (inventory(), result)));
    for (command, result) in steps {
        mock.expect_execute()
            .once()
            .in_sequence(&mut sequence)
            .with(mockall::predicate::eq(command))
            .return_once(|_| result);
    }
    install_batch(manager, names, mock, &|_| {})
}

#[test]
fn description_includes_manager() {
    for (manager, name, label) in [
        (PackageManager::Pacman, "git", "git (pacman)"),
        (PackageManager::Paru, "paru-bin", "paru-bin (paru)"),
        (PackageManager::Winget, "Git.Git", "Git.Git (winget)"),
    ] {
        let resource = PackageResource::new(name.into(), manager, Arc::new(MockExecutor::new()));
        assert_eq!(resource.description(), label);
    }
}

#[test]
fn state_from_installed_distinguishes_present_and_missing() {
    let manager = PackageManager::Pacman;
    let resource = PackageResource::new("git".into(), manager, Arc::new(MockExecutor::new()));
    for (names, expected) in [
        (
            HashSet::from(["git".into(), "vim".into()]),
            ResourceState::Correct,
        ),
        (HashSet::new(), ResourceState::Missing),
    ] {
        let installed = InstalledPackages::new(manager, names);
        assert_eq!(resource.state_from_installed(&installed), expected);
    }
}

// ------------------------------------------------------------------
// get_installed_packages
// ------------------------------------------------------------------

#[test]
fn get_installed_pacman_parses_name_version_lines() {
    let mut mock = MockExecutor::new();
    mock.expect_execute().once().returning(|_| {
        Ok(ExecResult::success(
            "git 2.39.0\nvim 9.0.0\nbase-devel 1.0\n",
        ))
    });
    let installed = get_installed_packages(PackageManager::Pacman, &mock).unwrap();
    assert_eq!(
        installed,
        InstalledPackages::new(
            PackageManager::Pacman,
            HashSet::from(["git".into(), "vim".into(), "base-devel".into()])
        )
    );
}

#[test]
fn inventory_failures_are_not_reported_as_an_empty_package_set() {
    for manager in [
        PackageManager::Pacman,
        PackageManager::Paru,
        PackageManager::Winget,
    ] {
        for spawn_failure in [false, true] {
            let mut mock = MockExecutor::new();
            mock.expect_execute().once().returning(move |spec| {
                let program = if manager == PackageManager::Winget {
                    mock_inventory(&spec, &[]);
                    "winget"
                } else {
                    assert_eq!(spec.arguments(), ["-Q"]);
                    "pacman"
                };
                assert_eq!(spec.program(), program);
                assert_eq!(spec.working_dir(), None);
                assert!(spec.is_checked());
                if spawn_failure {
                    Err(ExecError::spawn(
                        program,
                        std::io::Error::other("fixture inventory failure"),
                    ))
                } else {
                    Err(ExecError::non_zero(
                        program,
                        ExecResult::failure(
                            "partial-package 1.0",
                            "fixture inventory failure",
                            Some(42),
                        ),
                    ))
                }
            });
            let error = get_installed_packages(manager, &mock).unwrap_err();
            assert!(
                format!("{error:#}").contains("fixture inventory failure"),
                "{manager}, spawn={spawn_failure}: {error:#}"
            );
            if !spawn_failure {
                assert!(format!("{error:#}").contains("42"), "{error:#}");
            }
            assert!(
                error.downcast_ref::<ExecError>().is_some(),
                "{manager} inventory must preserve the executor's failure classification"
            );
        }
    }
}

#[test]
fn get_installed_winget_parses_id_tokens() {
    let mut mock = MockExecutor::new();
    mock.expect_execute().once().returning(|spec| {
        mock_inventory(&spec, &["Git.Git", "Microsoft.PowerShell"]);
        Ok(ExecResult::success(""))
    });
    let installed = get_installed_packages(PackageManager::Winget, &mock).unwrap();
    assert!(installed.contains_package("Git.Git"));
    assert!(installed.contains_package("Microsoft.PowerShell"));
    assert!(
        !installed.contains_package("Git"),
        "display names should not be included"
    );
}

#[test]
fn get_installed_winget_ignores_export_metadata() {
    let stdout = r#"{
        "$schema": "https://aka.ms/winget-packages.schema.2.0.json",
        "CreationDate": "2026-11-16T00:00:00.000-00:00",
        "WinGetVersion": "1.12",
        "Sources": [{"SourceDetails": {"Name": "winget"}, "Packages": [
            {"PackageIdentifier": "Git.Git", "Version": "2.45.1"},
            {"PackageIdentifier": "Microsoft.WindowsTerminal", "Scope": "user"}
        ]}]
    }"#;

    let installed = parse_winget_ids(stdout).unwrap();
    assert!(installed.contains("Git.Git"));
    assert!(installed.contains("Microsoft.WindowsTerminal"));
    assert_eq!(installed.len(), 2, "only package IDs should be collected");
}

#[test]
fn get_installed_winget_does_not_depend_on_localized_names() {
    let stdout = r#"{"Sources":[{"Packages":[
        {"Name": "中文名称 App", "PackageIdentifier": "Unicode.App"},
        {"Name": "App — Edition", "PackageIdentifier": "App.Edition"},
        {"Name": "Ünïcödé App", "PackageIdentifier": "Unicode.Accented"}
    ]}]}"#;

    let installed = parse_winget_ids(stdout).unwrap();
    assert!(
        installed.contains("Unicode.App"),
        "should extract ID from row with CJK characters in Name"
    );
    assert!(
        installed.contains("App.Edition"),
        "should extract ID from row with em dash in Name"
    );
    assert!(
        installed.contains("Unicode.Accented"),
        "should extract ID from row with accented characters in Name"
    );
    assert_eq!(
        installed.len(),
        3,
        "all three package IDs should be collected"
    );
}

// ------------------------------------------------------------------
// PackageResource::apply
// ------------------------------------------------------------------

#[test]
fn native_apply_returns_applied_on_success() {
    for (manager, name) in [
        (PackageManager::Pacman, "git"),
        (PackageManager::Paru, "paru-bin"),
    ] {
        let mut mock = MockExecutor::new();
        if manager == PackageManager::Pacman {
            expect_sudo_lookup_if_needed(&mut mock);
        }
        mock.expect_execute()
            .once()
            .with(mockall::predicate::eq(native_install(manager, &[name])))
            .returning(|_| Ok(ExecResult::success("")));
        let resource = PackageResource::new(name.into(), manager, Arc::new(mock));
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }
}

#[test]
fn apply_preserves_executor_failure_types() {
    for manager in [
        PackageManager::Pacman,
        PackageManager::Paru,
        PackageManager::Winget,
    ] {
        for (exec_error, cancelled) in [
            (
                ExecError::Cancelled {
                    command: "fixture install".into(),
                    result: ExecResult::failure("", "", None),
                },
                true,
            ),
            (
                ExecError::spawn(
                    "fixture install",
                    std::io::Error::other("fixture spawn failure"),
                ),
                false,
            ),
        ] {
            let mut mock = MockExecutor::new();
            if manager == PackageManager::Pacman {
                expect_sudo_lookup_if_needed(&mut mock);
            }
            mock.expect_execute()
                .once()
                .return_once(move |_| Err(exec_error));
            let resource = PackageResource::new("fixture".into(), manager, Arc::new(mock));

            let error = resource.apply().unwrap_err();

            assert!(matches!(
                error,
                crate::engine::resource::ResourceError::Exec { .. }
            ));
            assert_eq!(error.is_cancelled(), cancelled, "{manager}: {error}");
            assert_eq!(
                error.category(),
                if cancelled {
                    "cancelled"
                } else {
                    "command_failed"
                },
                "{manager}: {error}",
            );
        }
    }
}

// ------------------------------------------------------------------
// install_missing_packages
// ------------------------------------------------------------------

#[test]
fn empty_install_batch_never_queries_or_invokes_a_package_manager() {
    for manager in [
        PackageManager::Pacman,
        PackageManager::Paru,
        PackageManager::Winget,
    ] {
        let report = install_missing_packages(manager, &[], &MockExecutor::new(), &|_| {
            panic!("an empty batch must not announce package work");
        })
        .unwrap();

        assert_eq!(report.applied_count(), 0, "{manager}");
        assert_eq!(report.already_correct_count(), 0, "{manager}");
        assert!(!report.has_failures(), "{manager}");
    }
}

#[test]
fn batch_install_announces_each_package_before_execution() {
    for manager in [PackageManager::Pacman, PackageManager::Paru] {
        for fails in [false, true] {
            let announced = Arc::new(std::sync::Mutex::new(Vec::new()));
            let announced_at_execute = Arc::clone(&announced);
            let mut mock = MockExecutor::new();
            if manager == PackageManager::Pacman {
                expect_sudo_lookup_if_needed(&mut mock);
            }
            mock.expect_execute()
                .once()
                .withf(|spec| spec.arguments() != ["-Q"])
                .returning(move |_| {
                    assert_eq!(
                        *announced_at_execute.lock().unwrap(),
                        ["first", "second"],
                        "all package names must be announced before the batch starts"
                    );
                    if fails {
                        Err(ExecError::spawn(
                            "package-manager",
                            std::io::Error::other("simulated failure"),
                        ))
                    } else {
                        Ok(ExecResult::success(""))
                    }
                });
            if !fails {
                mock.expect_execute()
                    .once()
                    .withf(|spec| {
                        spec.program() == "pacman"
                            && spec.arguments() == ["-Q"]
                            && spec.is_checked()
                    })
                    .returning(|_| Ok(ExecResult::success("first 1.0\nsecond 1.0\n")));
            }
            let result = install_batch(manager, &["first", "second"], mock, &|package| {
                announced.lock().unwrap().push(package.to_string());
            });

            assert_eq!(result.is_err(), fails);
            if let Ok(report) = result {
                assert_eq!(report.applied_count(), 2);
            }
            assert_eq!(
                *announced.lock().unwrap(),
                ["first", "second"],
                "each package must be announced exactly once"
            );
        }
    }
}

#[test]
fn native_batch_install_verifies_one_checked_transaction_with_one_inventory() {
    for (manager, names) in [
        (PackageManager::Pacman, ["git", "vim"]),
        (PackageManager::Paru, ["paru-bin", "yay"]),
    ] {
        let after = format!("{} 1.0\n{} 1.0\n", names[0], names[1]);
        let report = native_batch(
            manager,
            &names,
            Ok(ExecResult::success("")),
            Some(Ok(ExecResult::success(after))),
        )
        .unwrap();
        assert_eq!(report.applied_count(), 2, "{manager}");
        assert!(!report.has_failures(), "{manager}");
    }
}

#[test]
fn successful_native_batches_report_observed_presence_and_propagate_inventory_errors() {
    for manager in [PackageManager::Pacman, PackageManager::Paru] {
        for installed in [
            Some(""),
            Some("first 1.0\n"),
            Some("first 1.0\nsecond 2.0\n"),
            None,
        ] {
            let after = installed
                .map(ExecResult::success)
                .ok_or_else(|| ExecError::Cancelled {
                    command: "pacman -Q".into(),
                    result: ExecResult::failure("partial", "cancelled fixture", None),
                });
            let result = native_batch(
                manager,
                &["first", "second"],
                Ok(ExecResult::success("")),
                Some(after),
            );
            if let Some(installed) = installed {
                let report = result.unwrap();
                let expected = installed.lines().count();
                assert_eq!(report.applied_count(), expected);
                assert_eq!(report.failures().len(), 2 - expected);
                assert!(
                    report
                        .failures()
                        .iter()
                        .all(|failure| failure.reason.contains("still absent"))
                );
            } else {
                let error = result.unwrap_err();
                assert!(error.downcast_ref::<ExecError>().unwrap().is_cancelled());
                assert!(format!("{error:#}").contains("verifying installed packages"));
                let report = error.downcast_ref::<crate::engine::BatchReport>().unwrap();
                assert_eq!(
                    report.completion(),
                    crate::engine::BatchCompletion::Interrupted
                );
                assert_eq!(report.interrupted_count(), 2);
                assert_eq!(report.stats().failed_count(), 0);
            }
        }
    }
}

#[test]
fn batch_install_propagates_pacman_error() {
    let error = ExecError::spawn("pacman", std::io::Error::other("simulated failure"));
    assert!(native_batch(PackageManager::Pacman, &["git"], Err(error), None).is_err());
}

#[test]
fn failed_native_batch_reports_packages_that_were_actually_installed() {
    for manager in [PackageManager::Pacman, PackageManager::Paru] {
        for installed in ["", "first 1.0\n", "first 1.0\nsecond 2.0\n"] {
            let transaction_error = ExecError::non_zero(
                "fixture batch install",
                ExecResult::failure("", "fixture transaction failure", Some(1)),
            );
            let result = native_batch(
                manager,
                &["first", "second"],
                Err(transaction_error),
                Some(Ok(ExecResult::success(installed))),
            );

            if installed.contains("second") {
                let error = result.expect_err("a failed post-transaction hook must stay failed");
                assert!(format!("{error:#}").contains("fixture transaction failure"));
                let report = error.downcast_ref::<crate::engine::BatchReport>().unwrap();
                assert_eq!(report.completion(), crate::engine::BatchCompletion::Failed);
                assert_eq!(report.stats().changed_count(), 2);
                assert_eq!(report.stats().failed_count(), 0);
            } else {
                let report = result.unwrap();
                let expected_applied = usize::from(!installed.is_empty());
                assert_eq!(report.applied_count(), expected_applied, "{manager}");
                assert_eq!(report.failures().len(), 2 - expected_applied, "{manager}");
                assert!(
                    report.failures().iter().all(|failure| {
                        failure.reason.contains("fixture transaction failure")
                            && (expected_applied == 0 || failure.package == "second")
                    }),
                    "{report:?}"
                );
            }
        }
    }
}

#[test]
fn failed_batch_inventory_errors_remain_errors_not_partial_success() {
    for cancelled in [false, true] {
        let batch_failure = ExecError::non_zero(
            "fixture batch",
            ExecResult::failure("", "fixture batch failure", Some(1)),
        );
        let result = ExecResult::failure("first 1.0\n", "fixture query failure", Some(1));
        let query_failure = if cancelled {
            ExecError::Cancelled {
                command: "pacman -Q".into(),
                result,
            }
        } else {
            ExecError::non_zero("pacman -Q", result)
        };
        let error = native_batch(
            PackageManager::Paru,
            &["first", "second"],
            Err(batch_failure),
            Some(Err(query_failure)),
        )
        .unwrap_err();

        let message = format!("{error:#}");
        assert!(message.contains("fixture batch failure"), "{message}");
        assert!(message.contains("fixture query failure"), "{message}");
        let report = error.downcast_ref::<crate::engine::BatchReport>().unwrap();
        assert_eq!(report.interrupted_count(), if cancelled { 2 } else { 0 });
        assert_eq!(report.stats().failed_count(), 0);
        assert_eq!(
            error
                .downcast_ref::<ExecError>()
                .is_some_and(ExecError::is_cancelled),
            cancelled,
        );
    }
}

#[test]
fn winget_install_report_continues_after_individual_failures_without_losing_outcomes() {
    for (case, failure) in [
        (
            "installer failure",
            Ok(ExecResult::failure(
                "",
                "fixture installer failure",
                Some(1),
            )),
        ),
        (
            "spawn failure",
            Err(ExecError::spawn(
                "winget",
                std::io::Error::other("fixture installer failure"),
            )),
        ),
        (
            "timeout",
            Err(ExecError::TimedOut {
                command: "winget".into(),
                timeout: std::time::Duration::from_secs(30),
                result: ExecResult::failure("", "fixture installer failure", None),
            }),
        ),
    ] {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let names = ["First.App", "Second.App", "Already.Present"];
        let mut mock = MockExecutor::new();
        let mut sequence = mockall::Sequence::new();
        for (name, outcome) in names.into_iter().zip([
            failure,
            Ok(ExecResult::success("")),
            Ok(ExecResult::failure("", "", Some(-1_978_335_189))),
        ]) {
            let recorded = Arc::clone(&events);
            mock.expect_execute()
                .once()
                .in_sequence(&mut sequence)
                .with(mockall::predicate::eq(winget_install(name)))
                .return_once(move |_| {
                    recorded.lock().unwrap().push(format!("install:{name}"));
                    outcome
                });
        }
        let report = install_batch(PackageManager::Winget, &names, mock, &|package| {
            events.lock().unwrap().push(format!("progress:{package}"));
        })
        .unwrap();

        assert_eq!(report.applied_count(), 1, "{case}");
        assert_eq!(report.already_correct_count(), 1, "{case}");
        assert_eq!(report.failures().len(), 1, "{case}");
        let recorded_failure = &report.failures()[0];
        assert_eq!(recorded_failure.package, "First.App", "{case}");
        assert!(
            recorded_failure
                .reason
                .contains("fixture installer failure"),
            "{case}: {recorded_failure:?}"
        );
        assert_eq!(
            *events.lock().unwrap(),
            [
                "progress:First.App",
                "install:First.App",
                "progress:Second.App",
                "install:Second.App",
                "progress:Already.Present",
                "install:Already.Present",
            ],
            "{case}: announce each package immediately before installing it"
        );
    }
}

#[test]
fn install_report_counts_already_correct_packages_separately_from_applied() {
    #[derive(Debug)]
    struct AlreadyCorrectProvider;

    impl PackageProvider for AlreadyCorrectProvider {
        fn name(&self) -> &'static str {
            "already-correct"
        }

        fn query_installed(&self, _executor: &dyn Executor) -> Result<InstalledPackages> {
            panic!("individual installs must not re-query inventory")
        }

        fn install(&self, _name: &str, _executor: &dyn Executor) -> Result<ResourceChange> {
            Ok(ResourceChange::AlreadyCorrect)
        }
    }

    let executor: Arc<dyn Executor> = Arc::new(MockExecutor::new());
    let resource = PackageResource::new(
        "Already.Present".to_string(),
        PackageManager::Winget,
        Arc::clone(&executor),
    );

    let report = AlreadyCorrectProvider
        .install_missing(&[&resource], &*executor, &|_| {})
        .unwrap();

    assert_eq!(report.applied_count(), 0);
    assert_eq!(report.already_correct_count(), 1);
    assert!(!report.has_failures());
}

#[test]
fn cancellation_retains_completed_packages_and_prior_failures() {
    #[derive(Debug)]
    struct CancellingProvider {
        first: ResourceChange,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl PackageProvider for CancellingProvider {
        fn name(&self) -> &'static str {
            "cancelling"
        }

        fn query_installed(&self, _executor: &dyn Executor) -> Result<InstalledPackages> {
            panic!("installation must not query this fixture")
        }

        fn install(&self, _name: &str, _executor: &dyn Executor) -> Result<ResourceChange> {
            match self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
                0 => Ok(self.first.clone()),
                1 => Err(ExecError::Cancelled {
                    command: "package install".into(),
                    result: ExecResult::failure("", "", None),
                }
                .into()),
                _ => panic!("cancellation must stop dispatch"),
            }
        }
    }

    for (first, expected) in [
        (ResourceChange::Applied, (1, 0, 0)),
        (ResourceChange::AlreadyCorrect, (0, 1, 0)),
        (ResourceChange::unusable("first failed"), (0, 0, 1)),
    ] {
        let provider = CancellingProvider {
            first,
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let executor: Arc<dyn Executor> = Arc::new(MockExecutor::new());
        let resources = ["first", "second", "third"].map(|name| {
            PackageResource::new(name.into(), PackageManager::Winget, Arc::clone(&executor))
        });
        let error = provider
            .install_missing(&resources.iter().collect::<Vec<_>>(), &*executor, &|_| {})
            .unwrap_err();
        let report = error.downcast_ref::<crate::engine::BatchReport>().unwrap();
        let stats = report.stats();
        assert_eq!(
            (
                stats.changed_count(),
                stats.already_ok_count(),
                stats.failed_count()
            ),
            expected
        );
        assert_eq!(report.interrupted_count(), 1);
        assert_eq!(report.not_attempted_count(), 1);
        assert_eq!(
            report.completion(),
            if expected.2 == 0 {
                crate::engine::BatchCompletion::Interrupted
            } else {
                assert!(format!("{error:#}").contains("first failed"));
                crate::engine::BatchCompletion::Failed
            }
        );
        assert!(error.downcast_ref::<ExecError>().unwrap().is_cancelled());
    }
}

#[test]
fn install_report_propagates_cancellation_without_starting_later_packages() {
    #[derive(Debug)]
    struct CancellingProvider {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl PackageProvider for CancellingProvider {
        fn name(&self) -> &'static str {
            "cancelling"
        }

        fn query_installed(&self, _executor: &dyn Executor) -> Result<InstalledPackages> {
            panic!("individual installs must not re-query inventory")
        }

        fn install(&self, _name: &str, _executor: &dyn Executor) -> Result<ResourceChange> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(ExecError::Cancelled {
                command: "package install".to_string(),
                result: ExecResult::failure("", "", Some(1)),
            }
            .into())
        }
    }

    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let provider = CancellingProvider {
        calls: Arc::clone(&calls),
    };
    let executor: Arc<dyn Executor> = Arc::new(MockExecutor::new());
    let first = PackageResource::new(
        "First.App".to_string(),
        PackageManager::Winget,
        Arc::clone(&executor),
    );
    let second = PackageResource::new(
        "Second.App".to_string(),
        PackageManager::Winget,
        Arc::clone(&executor),
    );

    let err = provider
        .install_missing(&[&first, &second], &*executor, &|_| {})
        .expect_err("cancellation must stop per-package installation");

    assert!(
        err.downcast_ref::<ExecError>()
            .is_some_and(ExecError::is_cancelled)
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}
