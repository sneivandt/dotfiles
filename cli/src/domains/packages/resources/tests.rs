//! Tests for package resources and providers.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;

use super::package::*;
use super::winget::parse_winget_ids;
use crate::engine::{Resource, ResourceChange, ResourceState};
use crate::infra::exec::{CommandSpec, ExecError, ExecResult, Executor, MockExecutor};

#[cfg(unix)]
fn running_as_root() -> bool {
    nix::unistd::Uid::effective().is_root()
}

#[cfg(not(unix))]
const fn running_as_root() -> bool {
    false
}

fn expect_sudo_lookup_if_needed(mock: &mut MockExecutor) {
    if !running_as_root() {
        mock.expect_which()
            .once()
            .with(mockall::predicate::eq("sudo"))
            .returning(|_| true);
    }
}

#[test]
fn description_includes_manager() {
    let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::ProcessExecutor::system());
    let pacman_resource = PackageResource::new(
        "git".to_string(),
        PackageManager::Pacman,
        Arc::clone(&executor),
    );
    assert_eq!(pacman_resource.description(), "git (pacman)");

    let paru_resource = PackageResource::new(
        "paru-bin".to_string(),
        PackageManager::Paru,
        Arc::clone(&executor),
    );
    assert_eq!(paru_resource.description(), "paru-bin (paru)");

    let winget_resource = PackageResource::new(
        "Git.Git".to_string(),
        PackageManager::Winget,
        Arc::clone(&executor),
    );
    assert_eq!(winget_resource.description(), "Git.Git (winget)");
}

#[test]
fn state_from_installed_correct() {
    let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::ProcessExecutor::system());
    let resource = PackageResource::new(
        "git".to_string(),
        PackageManager::Pacman,
        Arc::clone(&executor),
    );
    let mut installed = HashSet::new();
    installed.insert("git".to_string());
    installed.insert("vim".to_string());
    assert_eq!(
        resource.state_from_installed(&installed),
        ResourceState::Correct
    );
}

#[test]
fn state_from_installed_missing() {
    let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::ProcessExecutor::system());
    let resource = PackageResource::new(
        "git".to_string(),
        PackageManager::Pacman,
        Arc::clone(&executor),
    );
    let installed = HashSet::new();
    assert_eq!(
        resource.state_from_installed(&installed),
        ResourceState::Missing
    );
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
        HashSet::from(["git".into(), "vim".into(), "base-devel".into()])
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
                let (program, args) = if manager == PackageManager::Winget {
                    (
                        "winget",
                        vec![
                            "list",
                            "--accept-source-agreements",
                            "--disable-interactivity",
                        ],
                    )
                } else {
                    ("pacman", vec!["-Q"])
                };
                assert_eq!(spec.program(), program);
                assert_eq!(spec.arguments(), args);
                assert_eq!(spec.working_dir(), None);
                assert!(!spec.is_checked());
                if spawn_failure {
                    Err(ExecError::spawn(
                        program,
                        std::io::Error::other("fixture inventory failure"),
                    ))
                } else {
                    Ok(ExecResult::failure(
                        "partial-package 1.0",
                        "fixture inventory failure",
                        Some(42),
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
        }
    }
}

#[test]
fn get_installed_winget_parses_id_tokens() {
    let mut mock = MockExecutor::new();
    mock.expect_execute().once().returning(|_| {
            Ok(ExecResult::success(
                "Name          Id                    Version\nGit           Git.Git               2.39.0\nPowerShell    Microsoft.PowerShell  7.3\n",
            ))
        });
    let installed = get_installed_packages(PackageManager::Winget, &mock).unwrap();
    assert!(installed.contains("Git.Git"));
    assert!(installed.contains("Microsoft.PowerShell"));
    assert!(
        !installed.contains("Git"),
        "display names should not be included"
    );
}

#[test]
fn get_installed_winget_ignores_separator_and_extra_columns() {
    let stdout = concat!(
        "Name                          Id                           Version        Available Source\n",
        "-------------------------------------------------------------------------------------\n",
        "Git                           Git.Git                      2.45.1         2.46.0   winget\n",
        "Windows Terminal              Microsoft.WindowsTerminal    1.21.2361.0             winget\n",
    );

    let installed = parse_winget_ids(stdout).unwrap();
    assert!(installed.contains("Git.Git"));
    assert!(installed.contains("Microsoft.WindowsTerminal"));
    assert_eq!(installed.len(), 2, "only package IDs should be collected");
}

#[test]
fn get_installed_winget_handles_unicode_in_name_column() {
    // The Name column contains multi-byte and multi-width Unicode characters
    // (accented letters, an em dash, and wide CJK characters). winget aligns the
    // Id column by display width, not byte or char offset, so the parser must
    // slice columns by display column to extract IDs from every row.
    let stdout = concat!(
        "Name                          Id                           Version\n",
        "-------------------------------------------------------------------\n",
        // CJK characters (display width 2 each) in Name
        "中文名称 App                  Unicode.App                  1.0.0\n",
        // em dash (display width 1) in Name
        "App \u{2014} Edition                 App.Edition                  2.0.0\n",
        // accented characters (display width 1 each) in Name
        "Ünïcödé App                   Unicode.Accented             3.0.0\n",
    );

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
fn apply_pacman_returns_applied_on_success() {
    let mut mock = MockExecutor::new();
    expect_sudo_lookup_if_needed(&mut mock);
    mock.expect_execute()
        .once()
        .returning(|_| Ok(ExecResult::success("")));
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let resource = PackageResource::new(
        "git".to_string(),
        PackageManager::Pacman,
        Arc::clone(&executor),
    );
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
}

#[test]
fn apply_paru_returns_applied_on_success() {
    let mut mock = MockExecutor::new();
    mock.expect_execute().once().returning(|spec| {
        assert_eq!(spec.program(), "/usr/bin/paru");
        Ok(ExecResult::success(""))
    });
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let resource = PackageResource::new(
        "paru-bin".to_string(),
        PackageManager::Paru,
        Arc::clone(&executor),
    );
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
}

// ------------------------------------------------------------------
// install_missing_packages
// ------------------------------------------------------------------

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
            mock.expect_execute().once().returning(move |_| {
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
            let executor: Arc<dyn Executor> = Arc::new(mock);
            let first = PackageResource::new("first".to_string(), manager, Arc::clone(&executor));
            let second = PackageResource::new("second".to_string(), manager, Arc::clone(&executor));

            let result =
                install_missing_packages(manager, &[&first, &second], &*executor, &|package| {
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
fn native_batch_install_uses_one_checked_command() {
    let (pacman_program, pacman_args) = if running_as_root() {
        ("pacman", vec!["-Syu", "--needed", "--noconfirm"])
    } else {
        ("sudo", vec!["pacman", "-Syu", "--needed", "--noconfirm"])
    };
    for (manager, names, program, mut args) in [
        (
            PackageManager::Pacman,
            ["git", "vim"],
            pacman_program,
            pacman_args,
        ),
        (
            PackageManager::Paru,
            ["paru-bin", "yay"],
            "/usr/bin/paru",
            vec!["-S", "--needed", "--noconfirm"],
        ),
    ] {
        let mut mock = MockExecutor::new();
        if manager == PackageManager::Pacman {
            expect_sudo_lookup_if_needed(&mut mock);
        }
        args.extend(names);
        let args: Vec<_> = args.into_iter().map(std::ffi::OsString::from).collect();
        mock.expect_execute()
            .once()
            .withf(move |spec| {
                spec.program() == program
                    && spec.arguments() == args
                    && spec.is_checked()
                    && spec.working_dir().is_none()
            })
            .returning(|_| Ok(ExecResult::success("")));
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resources = names
            .map(|name| PackageResource::new(name.to_string(), manager, Arc::clone(&executor)));
        let references: Vec<_> = resources.iter().collect();

        let report = install_missing_packages(manager, &references, &*executor, &|_| {}).unwrap();

        assert_eq!(report.applied_count(), 2, "{manager}");
        assert!(!report.has_failures(), "{manager}");
    }
}

#[test]
fn inconsistent_provider_configs_are_rejected_before_commands_or_progress() {
    for manager in [
        PackageManager::Pacman,
        PackageManager::Paru,
        PackageManager::Winget,
    ] {
        let executor: Arc<dyn Executor> = Arc::new(MockExecutor::new());
        let first = PackageResource::new("first".into(), manager, Arc::clone(&executor))
            .with_provider_config("first.conf".into());
        let second = PackageResource::new("second".into(), manager, Arc::clone(&executor))
            .with_provider_config("second.conf".into());
        let error = install_missing_packages(manager, &[&first, &second], &*executor, &|_| {
            panic!("invalid batch must not announce an install")
        })
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "package batch contains inconsistent provider configuration",
            "{manager}"
        );
    }
}

#[test]
fn batch_install_propagates_pacman_error() {
    let mut mock = MockExecutor::new();
    expect_sudo_lookup_if_needed(&mut mock);
    mock.expect_execute().once().returning(|_| {
        Err(ExecError::spawn(
            "pacman",
            std::io::Error::other("simulated failure"),
        ))
    });
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let r1 = PackageResource::new(
        "git".to_string(),
        PackageManager::Pacman,
        Arc::clone(&executor),
    );
    assert!(install_missing_packages(PackageManager::Pacman, &[&r1], &*executor, &|_| {}).is_err());
}

#[test]
fn failed_native_batch_reports_packages_that_were_actually_installed() {
    for manager in [PackageManager::Pacman, PackageManager::Paru] {
        for installed in ["", "first 1.0\n", "first 1.0\nsecond 2.0\n"] {
            let mut mock = MockExecutor::new();
            if manager == PackageManager::Pacman {
                expect_sudo_lookup_if_needed(&mut mock);
            }
            let mut sequence = mockall::Sequence::new();
            mock.expect_execute()
                .once()
                .in_sequence(&mut sequence)
                .withf(CommandSpec::is_checked)
                .returning(|_| {
                    Err(ExecError::non_zero(
                        "fixture batch install",
                        ExecResult::failure("", "fixture transaction failure", Some(1)),
                    ))
                });
            mock.expect_execute()
                .once()
                .in_sequence(&mut sequence)
                .withf(|spec| {
                    spec.program() == "pacman" && spec.arguments() == ["-Q"] && !spec.is_checked()
                })
                .returning(move |_| Ok(ExecResult::success(installed)));
            let executor: Arc<dyn Executor> = Arc::new(mock);
            let first = PackageResource::new("first".into(), manager, Arc::clone(&executor));
            let second = PackageResource::new("second".into(), manager, Arc::clone(&executor));

            let result = install_missing_packages(manager, &[&first, &second], &*executor, &|_| {});

            if installed.contains("second") {
                let error = result.expect_err("a failed post-transaction hook must stay failed");
                assert!(error.to_string().contains("fixture transaction failure"));
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
        let mut mock = MockExecutor::new();
        let mut sequence = mockall::Sequence::new();
        mock.expect_execute()
            .once()
            .in_sequence(&mut sequence)
            .withf(CommandSpec::is_checked)
            .returning(|_| {
                Err(ExecError::non_zero(
                    "fixture batch",
                    ExecResult::failure("", "fixture batch failure", Some(1)),
                ))
            });
        mock.expect_execute()
            .once()
            .in_sequence(&mut sequence)
            .withf(|spec| {
                spec.program() == "pacman" && spec.arguments() == ["-Q"] && !spec.is_checked()
            })
            .returning(move |_| {
                let result = ExecResult::failure("first 1.0\n", "fixture query failure", Some(1));
                if cancelled {
                    Err(ExecError::Cancelled {
                        command: "pacman -Q".into(),
                        result,
                    })
                } else {
                    Ok(result)
                }
            });
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let first =
            PackageResource::new("first".into(), PackageManager::Paru, Arc::clone(&executor));
        let second =
            PackageResource::new("second".into(), PackageManager::Paru, Arc::clone(&executor));

        let error = install_missing_packages(
            PackageManager::Paru,
            &[&first, &second],
            &*executor,
            &|_| {},
        )
        .unwrap_err();

        let message = format!("{error:#}");
        assert!(message.contains("fixture batch failure"), "{message}");
        assert!(message.contains("fixture query failure"), "{message}");
        assert_eq!(
            error
                .downcast_ref::<ExecError>()
                .is_some_and(ExecError::is_cancelled),
            cancelled,
        );
    }
}

#[test]
fn winget_unmet_install_is_recorded_as_a_package_failure() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .returning(|_| Ok(ExecResult::failure("", "", Some(1))));
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let r1 = PackageResource::new(
        "Git.Git".to_string(),
        PackageManager::Winget,
        Arc::clone(&executor),
    );
    let report =
        install_missing_packages(PackageManager::Winget, &[&r1], &*executor, &|_| {}).unwrap();
    assert_eq!(report.applied_count(), 0);
    assert_eq!(report.failures().len(), 1);
    let failure = &report.failures()[0];
    assert_eq!(failure.package, "Git.Git");
    assert!(
        failure.reason.contains("winget install failed"),
        "unexpected failure: {failure:?}"
    );
}

#[test]
fn winget_install_report_tracks_successful_package_names() {
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|_| Ok(ExecResult::failure("", "", Some(1))));
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|_| Ok(ExecResult::success("")));

    let executor: Arc<dyn Executor> = Arc::new(mock);
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

    let announced = std::sync::Mutex::new(Vec::new());
    let report = install_missing_packages(
        PackageManager::Winget,
        &[&first, &second],
        &*executor,
        &|package| {
            announced.lock().unwrap().push(package.to_string());
        },
    )
    .unwrap();

    assert_eq!(
        *announced.lock().unwrap(),
        vec!["First.App".to_string(), "Second.App".to_string()],
        "each package is announced before it is installed",
    );
    assert_eq!(report.applied_count(), 1);
    assert_eq!(report.failures().len(), 1);
    assert_eq!(report.failures()[0].package, "First.App");
}

#[test]
fn install_report_counts_already_correct_packages_separately_from_applied() {
    #[derive(Debug)]
    struct AlreadyCorrectProvider;

    impl PackageProvider for AlreadyCorrectProvider {
        fn name(&self) -> &'static str {
            "already-correct"
        }

        fn query_installed(&self, _executor: &dyn Executor) -> Result<HashSet<String>> {
            Ok(HashSet::new())
        }

        fn install(
            &self,
            _name: &str,
            _executor: &dyn Executor,
            _config_path: Option<&str>,
        ) -> Result<ResourceChange> {
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
fn install_report_propagates_cancellation_without_starting_later_packages() {
    #[derive(Debug)]
    struct CancellingProvider {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl PackageProvider for CancellingProvider {
        fn name(&self) -> &'static str {
            "cancelling"
        }

        fn query_installed(&self, _executor: &dyn Executor) -> Result<HashSet<String>> {
            Ok(HashSet::new())
        }

        fn install(
            &self,
            _name: &str,
            _executor: &dyn Executor,
            _config_path: Option<&str>,
        ) -> Result<ResourceChange> {
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
