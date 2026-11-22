use super::*;

#[test]
fn aur_preview_uses_package_database_without_requiring_paru() {
    for query_fails in [false, true] {
        let packages = packages(&[("example-aur", true)]);
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "pacman" && spec.arguments() == ["-Q"] && spec.is_checked()
            })
            .returning(move |_| {
                if query_fails {
                    Err(ExecError::non_zero(
                        "pacman -Q",
                        ExecResult::failure("", "fixture database error", Some(1)),
                    ))
                } else {
                    Ok(ExecResult::success("Example-Aur 1.0\n"))
                }
            });
        let ctx = make_package_context(PathBuf::from("/tmp"), None, Os::Linux, true, mock)
            .with_dry_run(true);
        let result = InstallAurPackages::new(packages).run(&ctx);
        if query_fails {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("querying installed")
            );
        } else {
            assert_eq!(task_batch(&result.unwrap()).changed_count(), 1);
        }
    }
}

#[test]
fn package_task_applicability_follows_platform_and_package_kind() {
    for (label, os, arch, package_kinds, expected) in [
        (
            "empty Linux",
            Os::Linux,
            false,
            &[][..],
            [false, false, false],
        ),
        (
            "AUR on Arch",
            Os::Linux,
            true,
            &[true][..],
            [false, true, true],
        ),
        (
            "native on Linux",
            Os::Linux,
            false,
            &[false][..],
            [true, false, false],
        ),
        (
            "AUR on Linux",
            Os::Linux,
            false,
            &[true][..],
            [false, false, false],
        ),
        (
            "native on Arch",
            Os::Linux,
            true,
            &[false][..],
            [true, false, true],
        ),
        (
            "empty Windows",
            Os::Windows,
            false,
            &[][..],
            [false, false, false],
        ),
    ] {
        let entries = package_kinds
            .iter()
            .map(|&is_aur| Package {
                name: if is_aur { "paru-bin" } else { "git" }.to_string(),
                is_aur,
            })
            .collect();
        let packages = ConfigHandle::new(entries);
        let ctx = make_platform_context_with_which(PathBuf::from("/repo"), None, os, arch, false);
        let actual = [
            InstallPackages::new(packages.clone()).should_run(&ctx),
            InstallAurPackages::new(packages).should_run(&ctx),
            InstallParu.should_run(&ctx),
        ];
        assert_eq!(actual, expected, "{label}: [native, AUR, paru]");
    }
}

#[test]
fn package_elevation_requires_missing_packages_on_a_supported_platform() {
    for is_aur in [false, true] {
        for (os, arch, installed, expected) in [
            (Os::Linux, true, false, true),
            (Os::Linux, true, true, false),
            (Os::Linux, false, false, false),
            (Os::Windows, false, false, false),
        ] {
            let packages = ConfigHandle::new(vec![Package {
                name: "fixture-package".into(),
                is_aur,
            }]);
            let mut mock = MockExecutor::new();
            mock.expect_which().returning(|_| true);
            mock.expect_execute()
                .times(usize::from(arch))
                .withf(is_native_inventory)
                .returning(move |_| {
                    Ok(ExecResult::success(if installed {
                        "fixture-package 1.0\n"
                    } else {
                        "Fixture-Package 1.0\n"
                    }))
                });
            let ctx = make_package_context("/fixture".into(), None, os, arch, mock);
            let task: Box<dyn Task> = if is_aur {
                Box::new(InstallAurPackages::new(packages))
            } else {
                Box::new(InstallPackages::new(packages))
            };
            assert_eq!(
                task.needs_elevation(&ctx),
                expected,
                "{os:?}, Arch={arch}, AUR={is_aur}, installed={installed}"
            );
        }
    }
}

#[test]
fn unavailable_custom_package_preserves_failure_counts_and_diagnostics() {
    use crate::infra::logging::MsgKind;
    use crate::test_helpers::CapturingOutput;

    for (installed, other_install_fails) in [(false, false), (true, false), (false, true)] {
        let dir = tempfile::tempdir().unwrap();
        let recipe = dir.path().join("packages/local-tool");
        std::fs::create_dir_all(&recipe).unwrap();
        std::fs::write(recipe.join("release.json"), r#"{"package_base":"local-tool","repository":"example/tool","assets":{"x86_64":"tool.tar.gz"}}"#).unwrap();
        std::fs::write(recipe.join("PKGBUILD"), "pkgname=local-tool\n").unwrap();
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .withf(is_native_inventory)
            .returning(move |_| {
                Ok(ExecResult::success(if installed {
                    "local-tool 1.0\nexisting 1.0\n"
                } else {
                    "existing 1.0\n"
                }))
            });
        if other_install_fails {
            mock.expect_execute()
                .once()
                .withf(|spec| {
                    spec.program() == "/usr/bin/paru"
                        && spec.arguments() == ["-S", "--needed", "--noconfirm", "unrelated"]
                        && spec.is_checked()
                })
                .returning(|_| {
                    Err(ExecError::spawn(
                        "paru",
                        std::io::Error::other("fixture install failure"),
                    ))
                });
        }
        let output = Arc::new(CapturingOutput::default());
        let ctx = make_package_context(dir.path().into(), None, Os::Linux, true, mock)
            .with_home(dir.path().join("home"))
            .with_env(crate::infra::env::MapEnv::new().into_handle())
            .with_log(Arc::<CapturingOutput>::clone(&output));
        let mut names = vec!["local-tool", "existing"];
        if other_install_fails {
            names.push("unrelated");
        }
        let operation = PackageInstallOperation::new(
            names
                .into_iter()
                .map(|name| Package {
                    name: name.into(),
                    is_aur: true,
                })
                .collect(),
            PackageManager::Paru,
        );
        let result = process_operation(&ctx, &operation).unwrap();
        if installed {
            assert_task_ok(&result);
        } else {
            let stats = task_batch(&result);
            assert_eq!(
                stats.failed_count(),
                if other_install_fails { 2 } else { 1 }
            );
            assert_eq!(stats.changed_count(), 0);
            assert_eq!(stats.already_ok_count(), 1);
            assert!(
                output.messages_of(MsgKind::Warn).iter().any(|message| {
                    message
                        .contains("custom package local-tool unavailable; refusing AUR fallback:")
                        && message.contains("custom package release pins missing")
                }),
                "unavailable custom packages need an actionable diagnostic"
            );
        }
        assert!(
            !ctx.home().exists(),
            "availability checking must not prepare snapshots or config"
        );
    }
}

#[test]
fn consumers_are_released_only_when_inventory_and_paru_prove_no_package_work() {
    for case in [
        "current",
        "missing-native",
        "missing-aur",
        "different-case",
        "database-error",
        "broken-paru",
    ] {
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .withf(|program| program == "pacman")
            .return_const(true);
        mock.expect_execute()
            .once()
            .withf(|spec| spec.program() == "pacman" && spec.arguments() == ["-Q"])
            .returning(move |_| {
                if case == "database-error" {
                    return Err(ExecError::non_zero(
                        "pacman -Q",
                        ExecResult::failure("", "database failure", Some(1)),
                    ));
                }
                Ok(match case {
                    "missing-native" => ExecResult::success("aur-tool 1\n"),
                    "missing-aur" => ExecResult::success("native-tool 1\n"),
                    "different-case" => ExecResult::success("native-tool 1\nAur-Tool 1\n"),
                    _ => ExecResult::success("native-tool 1\naur-tool 1\n"),
                })
            });
        if matches!(case, "current" | "broken-paru") {
            expect_installed_paru_package(&mut mock, 1);
            if case == "current" {
                expect_healthy_paru(&mut mock, 1);
            } else {
                mock.expect_execute()
                    .once()
                    .withf(|spec| spec.program() == "/usr/bin/paru")
                    .returning(|_| {
                        Err(ExecError::spawn(
                            "paru",
                            std::io::Error::from(std::io::ErrorKind::NotFound),
                        ))
                    });
            }
        }
        let ctx = make_package_context(PathBuf::from("/tmp"), None, Os::Linux, true, mock);
        let state =
            package_stability_requirement(packages(&[("native-tool", false), ("aur-tool", true)]))
                .check(&ctx, crate::engine::RequirementStage::Ordering);
        match case {
            "current" => assert!(matches!(state, RequirementState::Satisfied(()))),
            "database-error" => assert!(matches!(state, RequirementState::Unknown { .. })),
            _ => assert!(
                matches!(state, RequirementState::Unsatisfied { .. }),
                "{case}: {state:?}"
            ),
        }
    }
}

#[test]
fn consumer_package_readiness_retains_waits_without_a_supported_manager() {
    for arch in [false, true] {
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .times(usize::from(arch))
            .withf(|program| program == "pacman")
            .return_const(false);
        // No inventory or health command may run without a supported manager.
        let ctx = make_package_context(PathBuf::from("/tmp"), None, Os::Linux, arch, mock);
        assert!(matches!(
            package_stability_requirement(packages(&[("tool", false)]))
                .check(&ctx, crate::engine::RequirementStage::Ordering),
            RequirementState::Unsatisfied { .. },
        ));
    }
}
