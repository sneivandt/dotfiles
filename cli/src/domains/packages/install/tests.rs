//! Unit tests for package install tasks.

use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::domains::packages::config::packages::Package;
use crate::infra::ConfigHandle;
use crate::infra::exec::{CommandSpec, ExecError, ExecResult, MockExecutor};
use crate::infra::platform::Os;
use crate::test_helpers::{
    assert_task_changed, assert_task_ok, empty_config, make_platform_context_with_which,
    task_batch, task_skipped,
};
use std::path::PathBuf;

#[test]
fn aur_preview_uses_package_database_without_requiring_paru() {
    for query_fails in [false, true] {
        let config = empty_config(PathBuf::from("/tmp"));
        let packages = ConfigHandle::new(vec![Package {
            name: "example-aur".into(),
            is_aur: true,
        }]);
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "pacman" && spec.arguments() == ["-Q"] && !spec.is_checked()
            })
            .returning(move |_| {
                Ok(if query_fails {
                    ExecResult::failure("", "fixture database error", Some(1))
                } else {
                    ExecResult::success("already-installed 1.0\n")
                })
            });
        let ctx = make_package_context(config, Os::Linux, true, mock).with_dry_run(true);
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
        let mut config = empty_config(PathBuf::from("/repo"));
        config.packages = package_kinds
            .iter()
            .map(|&is_aur| Package {
                name: if is_aur { "paru-bin" } else { "git" }.to_string(),
                is_aur,
            })
            .collect();
        let packages = ConfigHandle::new(config.packages.clone());
        let ctx = make_platform_context_with_which(config, os, arch, false);
        let actual = [
            InstallPackages::new(packages.clone()).should_run(&ctx),
            InstallAurPackages::new(packages).should_run(&ctx),
            InstallParu.should_run(&ctx),
        ];
        assert_eq!(actual, expected, "{label}: [native, AUR, paru]");
    }
}

// -----------------------------------------------------------------------
// run() — early-exit paths that do not require a real package manager
// -----------------------------------------------------------------------

#[test]
fn install_packages_run_reports_the_missing_native_manager() {
    for (os, package, manager) in [
        (Os::Linux, "git", "pacman"),
        (Os::Windows, "Git.Git", "winget"),
    ] {
        let mut config = empty_config(PathBuf::from("/repo"));
        config.packages.push(Package {
            name: package.to_string(),
            is_aur: false,
        });
        let packages = ConfigHandle::new(config.packages.clone());
        let ctx = make_platform_context_with_which(config, os, false, false);
        let result = InstallPackages::new(packages).run(&ctx).unwrap();
        let reason = task_skipped(&result);
        assert_eq!(reason, format!("{manager} not found"), "{os:?}");
    }
}

#[test]
fn installed_target_paru_is_accepted_when_host_path_lookup_would_miss() {
    let config = empty_config(PathBuf::from("/tmp"));
    let mut mock = MockExecutor::new();
    expect_installed_paru_package(&mut mock, 1);
    expect_healthy_paru(&mut mock, 1);
    let ctx = make_package_context(config, Os::Linux, true, mock);
    let result = InstallParu.run(&ctx).unwrap();
    assert_task_ok(&result);
}

#[test]
fn install_paru_run_returns_ok_when_already_installed_in_dry_run() {
    let config = empty_config(PathBuf::from("/tmp"));
    let mut mock = MockExecutor::new();
    expect_installed_paru_package(&mut mock, 1);
    expect_healthy_paru(&mut mock, 1);
    let mut ctx = make_package_context(config, Os::Linux, true, mock);
    ctx = ctx.with_dry_run(true);
    let result = InstallParu.run(&ctx).unwrap();
    assert_task_ok(&result);
}

#[test]
fn install_paru_run_returns_dry_run_when_not_installed_in_dry_run() {
    let config = empty_config(PathBuf::from("/tmp"));
    let mut mock = MockExecutor::new();
    expect_missing_paru_package(&mut mock, 1);
    expect_missing_paru_path(&mut mock, 1);
    let mut ctx = make_package_context(config, Os::Linux, true, mock);
    ctx = ctx.with_dry_run(true);
    let result = InstallParu.run(&ctx).unwrap();
    assert_task_changed(&result);
}

#[test]
fn install_paru_reports_change_and_cleans_its_fixture_build_directory() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let build_dir = fixture.path().join("paru-build");
    std::fs::create_dir(&build_dir).unwrap();
    std::fs::write(build_dir.join("stale-checkout"), "replace me").unwrap();
    let unrelated = fixture.path().join("unrelated");
    std::fs::create_dir(&unrelated).unwrap();
    std::fs::write(unrelated.join("keep"), "unrelated content").unwrap();
    let config = empty_config(fixture.path().to_path_buf());
    let mut mock = MockExecutor::new();
    let package_queries = Arc::new(AtomicUsize::new(0));
    mock.expect_execute()
        .times(2)
        .withf(is_paru_package_query)
        .returning(move |_| {
            if package_queries.fetch_add(1, Ordering::SeqCst) == 0 {
                return Ok(ExecResult::failure(
                    "",
                    "error: package 'paru' was not found",
                    Some(1),
                ));
            }
            Ok(ExecResult::success("paru 2.1.0-2\n"))
        });
    mock.expect_execute()
        .once()
        .withf(is_native_inventory)
        .returning(|_| Ok(ExecResult::success("base-devel 1.0\n")));
    mock.expect_which_path()
        .once()
        .with(mockall::predicate::eq("paru"))
        .returning(|_| anyhow::bail!("paru not found on PATH"));
    expect_paru_build_prerequisites(&mut mock);
    expect_paru_clone(&mut mock, &build_dir);
    mock.expect_execute()
        .once()
        .withf(is_makepkg(build_dir.clone()))
        .returning(|_| Ok(ExecResult::success("")));
    expect_healthy_paru(&mut mock, 1);

    let ctx = make_package_context(config, Os::Linux, true, mock);
    let result = run_paru_install(&ctx, &build_dir).unwrap();
    let stats = task_batch(&result);
    assert!(
        stats.changed_count() > 0 && stats.message() == Some("installed paru"),
        "expected changed result after paru install, got {result:?}"
    );
    assert!(
        !build_dir.exists(),
        "successful installation must clean its checkout"
    );
    assert_eq!(
        std::fs::read_to_string(unrelated.join("keep")).unwrap(),
        "unrelated content"
    );
}

#[test]
fn paru_preview_and_completed_state_preserve_existing_build_files() {
    for (label, installed, dry_run) in [
        ("already installed", true, false),
        ("installed preview", true, true),
        ("missing preview", false, true),
    ] {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let build_dir = fixture.path().join("paru-build");
        std::fs::create_dir(&build_dir).unwrap();
        let marker = build_dir.join("keep");
        std::fs::write(&marker, "existing checkout").unwrap();
        let config = empty_config(fixture.path().to_path_buf());
        let mut mock = MockExecutor::new();
        if installed {
            expect_installed_paru_package(&mut mock, 1);
            expect_healthy_paru(&mut mock, 1);
        } else {
            expect_missing_paru_package(&mut mock, 1);
            expect_missing_paru_path(&mut mock, 1);
        }
        let ctx = make_package_context(config, Os::Linux, true, mock).with_dry_run(dry_run);

        let result = run_paru_install(&ctx, &build_dir).unwrap();

        if installed {
            assert_task_ok(&result);
        } else {
            assert_task_changed(&result);
        }
        assert_eq!(
            std::fs::read_to_string(marker).unwrap(),
            "existing checkout",
            "{label}"
        );
    }
}

#[test]
fn failed_paru_clone_cleans_only_its_partial_checkout() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let build_dir = fixture.path().join("paru-build");
    let marker = fixture.path().join("keep");
    std::fs::write(&marker, "unrelated content").unwrap();
    let config = empty_config(fixture.path().to_path_buf());
    let mut mock = MockExecutor::new();
    expect_missing_paru_package(&mut mock, 1);
    expect_missing_paru_path(&mut mock, 1);
    expect_paru_build_prerequisites(&mut mock);
    let partial = build_dir.clone();
    mock.expect_execute()
        .once()
        .withf(is_git_clone(build_dir.clone()))
        .returning(move |_| {
            std::fs::create_dir(&partial).unwrap();
            std::fs::write(partial.join("partial"), "incomplete checkout").unwrap();
            Err(ExecError::non_zero(
                "git clone",
                ExecResult::failure("", "fixture clone failed", Some(128)),
            ))
        });
    let ctx = make_package_context(config, Os::Linux, true, mock);

    let error = run_paru_install(&ctx, &build_dir).unwrap_err();

    assert!(
        format!("{error:#}").contains("fixture clone failed"),
        "{error:#}"
    );
    assert!(
        !build_dir.exists(),
        "failed clone must clean its partial checkout"
    );
    assert_eq!(
        std::fs::read_to_string(marker).unwrap(),
        "unrelated content"
    );
}

#[test]
fn install_aur_packages_errors_when_paru_disappears_after_bootstrap() {
    let mut config = empty_config(PathBuf::from("/tmp"));
    config.packages.push(Package {
        name: "paru-bin".to_string(),
        is_aur: true,
    });
    let packages = ConfigHandle::new(config.packages.clone());
    let mut mock = MockExecutor::new();
    expect_missing_paru_package(&mut mock, 1);
    expect_missing_paru_path(&mut mock, 1);
    let ctx = make_package_context(config, Os::Linux, true, mock);
    let error = InstallAurPackages::new(packages).run(&ctx).unwrap_err();
    assert!(
        error.to_string().contains("became unavailable"),
        "expected explicit post-bootstrap health error, got {error:#}"
    );
}

fn paru_path() -> PathBuf {
    PathBuf::from("/usr/bin/paru")
}

fn cargo_path() -> PathBuf {
    PathBuf::from("/usr/bin/cargo")
}

fn is_paru_version(spec: &CommandSpec) -> bool {
    spec.program() == paru_path().as_os_str()
        && spec.arguments() == ["--version"]
        && spec.is_checked()
        && spec.working_dir().is_none()
}

fn is_paru_package_query(spec: &CommandSpec) -> bool {
    spec.program() == "pacman"
        && spec.arguments() == ["-Q", "paru"]
        && !spec.is_checked()
        && spec.working_dir().is_none()
}

fn is_git_clone(build_dir: PathBuf) -> impl Fn(&CommandSpec) -> bool {
    move |spec| {
        spec.program() == "git"
            && spec.arguments()
                == [
                    std::ffi::OsString::from("clone"),
                    std::ffi::OsString::from("https://aur.archlinux.org/paru.git"),
                    build_dir.as_os_str().to_owned(),
                ]
            && spec.is_checked()
            && spec.working_dir().is_none()
    }
}

fn expect_paru_clone(mock: &mut MockExecutor, build_dir: &Path) {
    let build_dir = build_dir.to_path_buf();
    mock.expect_execute()
        .once()
        .withf(is_git_clone(build_dir.clone()))
        .returning(move |_| {
            assert!(
                !build_dir.exists(),
                "clone must receive a clean fixture directory"
            );
            std::fs::create_dir(&build_dir).unwrap();
            std::fs::write(build_dir.join("PKGBUILD"), "fixture checkout").unwrap();
            Ok(ExecResult::success(""))
        });
}

fn is_cargo_version(spec: &CommandSpec) -> bool {
    spec.program() == cargo_path().as_os_str()
        && spec.arguments() == ["--version"]
        && spec.is_checked()
        && spec.working_dir().is_none()
}

fn is_makepkg(build_dir: PathBuf) -> impl Fn(&CommandSpec) -> bool {
    move |spec| {
        spec.program() == "makepkg"
            && spec.arguments() == ["-si", "--noconfirm"]
            && spec.is_checked()
            && spec.working_dir() == Some(build_dir.as_path())
            && matches!(spec.environment(), [(key, value)] if key == "MAKEFLAGS"
                && value.to_str().and_then(|value| value.strip_prefix("-j"))
                    .and_then(|jobs| jobs.parse::<usize>().ok()).is_some_and(|jobs| jobs > 0))
    }
}

fn expect_missing_paru_path(mock: &mut MockExecutor, times: usize) {
    mock.expect_which_path()
        .times(times)
        .with(mockall::predicate::eq("paru"))
        .returning(|_| anyhow::bail!("paru not found on PATH"));
}

fn expect_installed_paru_package(mock: &mut MockExecutor, times: usize) {
    mock.expect_execute()
        .times(times)
        .withf(is_paru_package_query)
        .returning(|_| Ok(ExecResult::success("paru 2.1.0-2\n")));
}

fn expect_missing_paru_package(mock: &mut MockExecutor, times: usize) {
    mock.expect_execute()
        .times(times)
        .withf(is_paru_package_query)
        .returning(|_| {
            Ok(ExecResult::failure(
                "",
                "error: package 'paru' was not found",
                Some(1),
            ))
        });
    mock.expect_execute()
        .times(times)
        .withf(is_native_inventory)
        .returning(|_| Ok(ExecResult::success("base-devel 1.0\n")));
}

fn expect_healthy_paru(mock: &mut MockExecutor, times: usize) {
    mock.expect_execute()
        .times(times)
        .withf(is_paru_version)
        .returning(|_| Ok(ExecResult::success("paru v2.1.0 - libalpm v16.0.1\n")));
}

#[test]
fn paru_health_marks_nonzero_executable_broken() {
    let mut mock = MockExecutor::new();
    expect_installed_paru_package(&mut mock, 1);
    mock.expect_execute()
        .once()
        .withf(is_paru_version)
        .returning(|_| {
            Err(ExecError::non_zero(
                "/usr/bin/paru --version",
                ExecResult::failure("", "unexpected failure", Some(1)),
            ))
        });

    let health = check_paru_health(&mock).unwrap();

    assert!(matches!(
        health,
        ParuHealth::Broken { path, reason }
            if path == paru_path() && reason.contains("unexpected failure")
    ));
}

#[test]
fn paru_health_preserves_missing_libalpm_failure() {
    let mut mock = MockExecutor::new();
    expect_installed_paru_package(&mut mock, 1);
    mock.expect_execute()
        .once()
        .withf(is_paru_version)
        .returning(|_| {
            Err(ExecError::non_zero(
                "/usr/bin/paru --version",
                ExecResult::failure(
                    "",
                    "paru: error while loading shared libraries: libalpm.so.15: cannot open shared object file: No such file or directory",
                    Some(127),
                ),
            ))
        });

    let health = check_paru_health(&mock).unwrap();

    assert!(matches!(
        health,
        ParuHealth::Broken { path, reason }
            if path == paru_path() && reason.contains("libalpm.so.15")
    ));
}

#[test]
fn paru_health_marks_path_executable_without_target_package_broken() {
    let mut mock = MockExecutor::new();
    expect_missing_paru_package(&mut mock, 1);
    mock.expect_which_path()
        .once()
        .with(mockall::predicate::eq("paru"))
        .returning(|_| Ok(PathBuf::from("/usr/local/bin/paru")));

    let health = check_paru_health(&mock).unwrap();

    assert!(matches!(
        health,
        ParuHealth::Broken { path, reason }
            if path == Path::new("/usr/local/bin/paru")
                && reason.contains("not backed by the target package database")
    ));
}

#[test]
fn paru_prerequisites_reject_missing_cargo_with_arch_guidance() {
    let config = empty_config(PathBuf::from("/tmp"));
    let mut mock = MockExecutor::new();
    expect_paru_native_build_tools(&mut mock);
    mock.expect_which_path()
        .once()
        .with(mockall::predicate::eq("cargo"))
        .returning(|_| anyhow::bail!("cargo not found on PATH"));

    let ctx = make_package_context(config, Os::Linux, true, mock);
    let error = check_prerequisites(&ctx).unwrap_err();
    let message = format!("{error:#}");

    assert!(message.contains("missing prerequisite: cargo"), "{message}");
    assert!(message.contains("pacman -Syu --needed rust"), "{message}");
}

#[test]
fn paru_inventory_failure_never_plans_a_bootstrap_or_rebuild() {
    for dry_run in [false, true] {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .withf(is_paru_package_query)
            .returning(|_| {
                Ok(ExecResult::failure(
                    "",
                    "could not open package database",
                    Some(1),
                ))
            });
        mock.expect_execute()
            .once()
            .withf(is_native_inventory)
            .returning(|_| {
                Ok(ExecResult::failure(
                    "",
                    "fixture unreadable package database",
                    Some(1),
                ))
            });
        let config = empty_config(PathBuf::from("fixture-repository"));
        let ctx = make_package_context(config, Os::Linux, true, mock).with_dry_run(dry_run);

        let error = InstallParu.run(&ctx).unwrap_err();

        assert!(
            format!("{error:#}").contains("fixture unreadable package database"),
            "{error:#}"
        );
    }
}

#[test]
fn paru_targeted_query_failure_is_not_absence_when_inventory_contains_paru() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .withf(is_paru_package_query)
        .returning(|_| Ok(ExecResult::failure("", "fixture query failure", Some(1))));
    mock.expect_execute()
        .once()
        .withf(is_native_inventory)
        .returning(|_| Ok(ExecResult::success("paru 2.1.0-2\n")));

    let error = check_paru_health(&mock).unwrap_err();

    assert!(format!("{error:#}").contains("fixture query failure"));
}

#[test]
fn paru_probe_errors_are_not_converted_to_rebuild_plans() {
    #[derive(Debug, Clone, Copy)]
    enum Failure {
        Cancelled,
        Timeout,
        Spawn,
        Capture,
    }

    for stage in ["package query", "executable"] {
        for failure in [
            Failure::Cancelled,
            Failure::Timeout,
            Failure::Spawn,
            Failure::Capture,
        ] {
            for dry_run in [false, true] {
                let mut mock = MockExecutor::new();
                if stage == "executable" {
                    expect_installed_paru_package(&mut mock, 1);
                }
                mock.expect_execute()
                    .once()
                    .withf(move |spec| {
                        if stage == "executable" {
                            is_paru_version(spec)
                        } else {
                            is_paru_package_query(spec)
                        }
                    })
                    .returning(move |_| {
                        let command = "fixture paru probe".to_string();
                        let result = ExecResult::failure("", "", None);
                        Err(match failure {
                            Failure::Cancelled => ExecError::Cancelled { command, result },
                            Failure::Timeout => ExecError::TimedOut {
                                command,
                                timeout: std::time::Duration::from_secs(1),
                                result,
                            },
                            Failure::Spawn => ExecError::spawn(
                                command,
                                std::io::Error::from(std::io::ErrorKind::PermissionDenied),
                            ),
                            Failure::Capture => ExecError::Io {
                                command,
                                operation: "capturing output",
                                source: std::io::Error::other("fixture capture failure"),
                            },
                        })
                    });
                let config = empty_config(PathBuf::from("fixture-repository"));
                let ctx = make_package_context(config, Os::Linux, true, mock).with_dry_run(dry_run);

                let error = InstallParu.run(&ctx).unwrap_err();

                let exec_error = error.downcast_ref::<ExecError>().unwrap();
                assert_eq!(
                    exec_error.is_cancelled(),
                    matches!(failure, Failure::Cancelled)
                );
                assert!(
                    format!("{error:#}").contains("fixture paru probe"),
                    "{stage}/{failure:?}: {error:#}"
                );
            }
        }
    }
}

#[test]
fn paru_probe_failure_does_not_request_elevation() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .withf(is_paru_package_query)
        .returning(|_| {
            Err(ExecError::spawn(
                "pacman",
                std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            ))
        });
    let config = empty_config(PathBuf::from("fixture-repository"));
    let ctx = make_package_context(config, Os::Linux, true, mock);

    assert!(!InstallParu.needs_elevation(&ctx));
}

#[test]
fn paru_prerequisites_reject_unconfigured_rustup_cargo() {
    let config = empty_config(PathBuf::from("/tmp"));
    let mut mock = MockExecutor::new();
    expect_paru_native_build_tools(&mut mock);
    expect_cargo_path(&mut mock);
    mock.expect_execute()
        .once()
        .withf(is_cargo_version)
        .returning(|_| {
            Err(ExecError::non_zero(
                "/usr/bin/cargo --version",
                ExecResult::failure(
                    "",
                    "rustup could not choose a version of cargo to run because no default is configured",
                    Some(1),
                ),
            ))
        });

    let ctx = make_package_context(config, Os::Linux, true, mock);
    let error = check_prerequisites(&ctx).unwrap_err();
    let message = format!("{error:#}");

    assert!(
        message.contains("Rust/Cargo prerequisite is incomplete"),
        "{message}"
    );
    assert!(message.contains("rustup default stable"), "{message}");
    assert!(message.contains("no default is configured"), "{message}");
}

#[test]
fn broken_paru_is_rebuilt_and_revalidated() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let build_dir = fixture.path().join("paru-build");
    let config = empty_config(fixture.path().to_path_buf());
    let mut mock = MockExecutor::new();
    expect_installed_paru_package(&mut mock, 2);
    let checks = Arc::new(AtomicUsize::new(0));
    mock.expect_execute()
        .times(2)
        .withf(is_paru_version)
        .returning(move |_| {
            if checks.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(ExecError::non_zero(
                    "/usr/bin/paru --version",
                    ExecResult::failure("", "libalpm.so.15 not found", Some(127)),
                ));
            }
            Ok(ExecResult::success("paru v2.1.0 - libalpm v16.0.1\n"))
        });
    expect_paru_build_prerequisites(&mut mock);
    expect_paru_clone(&mut mock, &build_dir);
    mock.expect_execute()
        .once()
        .withf(is_makepkg(build_dir.clone()))
        .returning(|_| Ok(ExecResult::success("")));

    let ctx = make_package_context(config, Os::Linux, true, mock);
    let result = run_paru_install(&ctx, &build_dir).unwrap();
    let stats = task_batch(&result);

    assert_eq!(stats.message(), Some("rebuilt paru"));
    assert!(
        !build_dir.exists(),
        "successful rebuild must clean its checkout"
    );
}

#[test]
fn failed_paru_rebuild_returns_clear_root_error() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let build_dir = fixture.path().join("paru-build");
    let config = empty_config(fixture.path().to_path_buf());
    let mut mock = MockExecutor::new();
    expect_failed_paru_rebuild(&mut mock, &build_dir);

    let ctx = make_package_context(config, Os::Linux, true, mock);
    let error = run_paru_install(&ctx, &build_dir).unwrap_err();
    let message = format!("{error:#}");

    assert!(message.contains("paru rebuild attempt failed"), "{message}");
    assert!(message.contains("package build failed"), "{message}");
    assert!(
        !build_dir.exists(),
        "failed rebuild must clean its checkout"
    );
}

#[test]
fn failed_paru_rebuild_blocks_aur_package_task() {
    use std::collections::HashMap;

    use crate::engine::graph::ResolvedTaskGraph;
    use crate::engine::{TaskAssessment, TaskId, TaskMeta};
    use crate::infra::logging::{Log, TaskStatus};

    struct FixtureParuTask {
        inner: InstallParu,
        build_dir: PathBuf,
    }

    impl Task for FixtureParuTask {
        fn meta(&self) -> TaskMeta<'_> {
            self.inner.meta()
        }

        fn task_id(&self) -> TaskId {
            // Preserve the real AUR dependency while injecting only the checkout path.
            self.inner.task_id()
        }

        fn run(&self, ctx: &Context) -> Result<TaskResult> {
            run_paru_install(ctx, &self.build_dir)
        }
    }

    let fixture = tempfile::tempdir_in(".").unwrap();
    let build_dir = fixture.path().join("paru-build");
    let mut config = empty_config(fixture.path().to_path_buf());
    config.packages.push(Package {
        name: "apm-bin".to_string(),
        is_aur: true,
    });
    let packages = ConfigHandle::new(config.packages.clone());
    let mut mock = MockExecutor::new();
    expect_failed_paru_rebuild(&mut mock, &build_dir);

    let (log, _tmp, _guard) = crate::infra::logging::isolated_logger();
    let log = Arc::new(log);
    let log_output: Arc<dyn Log> = Arc::<crate::infra::logging::Logger>::clone(&log);
    let ctx = make_package_context(config, Os::Linux, true, mock).with_log(log_output);
    let install_paru = FixtureParuTask {
        inner: InstallParu,
        build_dir: build_dir.clone(),
    };
    let install_aur = InstallAurPackages::new(packages);
    let tasks: Vec<&dyn Task> = vec![&install_paru, &install_aur];
    let graph = ResolvedTaskGraph::resolve(&tasks).unwrap();
    let assessments = tasks
        .iter()
        .map(|task| (task.task_id(), TaskAssessment::applicable()))
        .collect::<HashMap<_, _>>();

    let summary =
        crate::engine::scheduler::run_tasks_sequential(&tasks, &graph, &assessments, &ctx, &log);

    assert_eq!(summary.failure_count(), 1);
    assert!(
        !build_dir.exists(),
        "scheduler failure must clean the fixture checkout"
    );
    let entries = log.task_entries();
    let paru_entry = entries
        .iter()
        .find(|entry| entry.name == "Paru package manager")
        .expect("paru task entry");
    assert_eq!(paru_entry.status, TaskStatus::Failed);
    assert!(
        paru_entry
            .message
            .as_deref()
            .is_some_and(|message| message.contains("paru rebuild attempt failed")),
        "root task should retain the rebuild failure: {paru_entry:?}"
    );
    let aur_entry = entries
        .iter()
        .find(|entry| entry.name == "AUR packages")
        .expect("AUR task entry");
    assert_eq!(aur_entry.status, TaskStatus::Blocked);
    assert_eq!(
        aur_entry.message.as_deref(),
        Some("blocked by failed dependency: Paru package manager")
    );

    let run_log = std::fs::read_to_string(log.log_path().expect("run log path")).unwrap();
    for diagnostic in [
        "paru status: broken",
        "executable /usr/bin/paru",
        "libalpm.so.15 not found",
        "paru rebuild attempted",
        "paru rebuild attempt failed",
    ] {
        assert!(
            run_log.contains(diagnostic),
            "run log should contain {diagnostic:?}:\n{run_log}"
        );
    }
}

fn expect_failed_paru_rebuild(mock: &mut MockExecutor, build_dir: &Path) {
    expect_installed_paru_package(mock, 1);
    mock.expect_execute()
        .once()
        .withf(is_paru_version)
        .returning(|_| {
            Err(ExecError::non_zero(
                "/usr/bin/paru --version",
                ExecResult::failure("", "libalpm.so.15 not found", Some(127)),
            ))
        });
    expect_paru_build_prerequisites(mock);
    expect_paru_clone(mock, build_dir);
    mock.expect_execute()
        .once()
        .withf(is_makepkg(build_dir.to_path_buf()))
        .returning(|_| {
            Err(ExecError::non_zero(
                "makepkg -si --noconfirm",
                ExecResult::failure("", "package build failed", Some(4)),
            ))
        });
}

fn expect_paru_native_build_tools(mock: &mut MockExecutor) {
    for dependency in ["git", "makepkg", "sudo"] {
        mock.expect_which()
            .once()
            .with(mockall::predicate::eq(dependency))
            .returning(|_| true);
    }
}

fn expect_cargo_path(mock: &mut MockExecutor) {
    mock.expect_which_path()
        .once()
        .with(mockall::predicate::eq("cargo"))
        .returning(|_| Ok(cargo_path()));
}

fn expect_working_cargo(mock: &mut MockExecutor) {
    expect_cargo_path(mock);
    mock.expect_execute()
        .once()
        .withf(is_cargo_version)
        .returning(|_| Ok(ExecResult::success("cargo 1.97.1\n")));
}

fn expect_paru_build_prerequisites(mock: &mut MockExecutor) {
    expect_paru_native_build_tools(mock);
    expect_working_cargo(mock);
}

// -----------------------------------------------------------------------
// run() — batch install paths (pacman/paru)
// -----------------------------------------------------------------------

/// Build a context that uses a [`MockExecutor`] with `which=true`.
///
/// This lets tests exercise the `process_packages` batch install path without
/// being short-circuited by the "tool not found" guard in `run()`.
fn make_package_context(
    config: crate::Config,
    os: Os,
    is_arch: bool,
    executor: MockExecutor,
) -> Context {
    use crate::infra::platform::Platform;
    crate::test_helpers::make_context(config, Platform::new(os, is_arch), Arc::new(executor))
}

fn is_native_inventory(spec: &CommandSpec) -> bool {
    spec.program() == "pacman"
        && spec.arguments() == ["-Q"]
        && !spec.is_checked()
        && spec.working_dir().is_none()
}

fn is_native_git_install(spec: &CommandSpec) -> bool {
    #[cfg(unix)]
    let root = nix::unistd::Uid::effective().is_root();
    #[cfg(not(unix))]
    let root = false;
    let (program, arguments) = if root {
        ("pacman", vec!["-Syu", "--needed", "--noconfirm", "git"])
    } else {
        (
            "sudo",
            vec!["pacman", "-Syu", "--needed", "--noconfirm", "git"],
        )
    };
    spec.program() == program
        && spec.arguments() == arguments
        && spec.is_checked()
        && spec.working_dir().is_none()
}

#[test]
fn install_packages_batch_installs_missing_packages_on_arch() {
    let root = tempfile::tempdir_in(".").unwrap();
    let mut config = empty_config(root.path().to_path_buf());
    config.packages.push(Package {
        name: "git".to_string(),
        is_aur: false,
    });
    config.packages.push(Package {
        name: "vim".to_string(),
        is_aur: false,
    });
    config.packages.push(Package {
        name: "aur-package-must-not-be-installed".to_string(),
        is_aur: true,
    });
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    mock.expect_which().returning(|_| true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .withf(is_native_inventory)
        .returning(|_| Ok(ExecResult::success("vim 9.0\n")));
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .withf(is_native_git_install)
        .returning(|_| Ok(ExecResult::success("")));
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_package_context(config, Os::Linux, true, mock);
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
    let mut config = empty_config(root.path().to_path_buf());
    config.packages.push(Package {
        name: "git".to_string(),
        is_aur: false,
    });
    // which("pacman") → true
    // run_unchecked("pacman", ["-Q"]) → git installed → no install needed
    let mut mock = MockExecutor::new();
    mock.expect_which().returning(|_| true);
    mock.expect_execute()
        .once()
        .withf(is_native_inventory)
        .returning(|_| Ok(ExecResult::success("git 2.40\n")));
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_package_context(config, Os::Linux, false, mock);
    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    assert_task_ok(&result);
}

#[test]
fn install_packages_dry_run_reports_missing_packages() {
    let root = tempfile::tempdir_in(".").unwrap();
    let mut config = empty_config(root.path().to_path_buf());
    config.packages.push(Package {
        name: "git".to_string(),
        is_aur: false,
    });
    // which("pacman") → true
    // run_unchecked("pacman", ["-Q"]) → nothing installed
    let mut mock = MockExecutor::new();
    mock.expect_which().returning(|_| true);
    mock.expect_execute()
        .once()
        .withf(is_native_inventory)
        .returning(|_| Ok(ExecResult::success("")));
    let packages = ConfigHandle::new(config.packages.clone());
    let mut ctx = make_package_context(config, Os::Linux, true, mock);
    ctx = ctx.with_dry_run(true);
    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    let stats = task_batch(&result);
    assert_eq!(
        (
            stats.changed_count(),
            stats.already_ok_count(),
            stats.failed_count()
        ),
        (1, 0, 0)
    );
}

#[test]
fn native_inventory_failure_prevents_planning_and_installation() {
    for dry_run in [false, true] {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let config = empty_config(fixture.path().to_path_buf());
        let packages = ConfigHandle::new(vec![Package {
            name: "git".into(),
            is_aur: false,
        }]);
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .once()
            .withf(|program| program == "pacman")
            .return_const(true);
        mock.expect_execute()
            .once()
            .withf(is_native_inventory)
            .returning(|_| {
                Ok(ExecResult::failure(
                    "partial-package 1.0\n",
                    "fixture corrupt database",
                    Some(42),
                ))
            });
        let ctx = make_package_context(config, Os::Linux, true, mock).with_dry_run(dry_run);
        let error = InstallPackages::new(packages).run(&ctx).unwrap_err();
        let details = format!("{error:#}");
        assert!(details.contains("fixture corrupt database"), "{details}");
        assert!(details.contains("42"), "{details}");
    }
}

#[test]
fn install_packages_returns_failed_when_batch_install_fails() {
    let root = tempfile::tempdir_in(".").unwrap();
    let mut config = empty_config(root.path().to_path_buf());
    config.packages.push(Package {
        name: "git".to_string(),
        is_aur: false,
    });
    // which("pacman") → true
    // run_unchecked("pacman", ["-Q"]) → git not installed
    // run("sudo", ["pacman", ...]) → error (simulating locked db)
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    mock.expect_which().returning(|_| true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .withf(is_native_inventory)
        .returning(|_| Ok(ExecResult::success("")));
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .withf(is_native_git_install)
        .returning(|_| {
            Err(ExecError::non_zero(
                "pacman",
                ExecResult::failure("", "database locked", Some(1)),
            ))
        });
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .withf(is_native_inventory)
        .returning(|_| Ok(ExecResult::success("")));
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_package_context(config, Os::Linux, true, mock);
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
    let config = empty_config(PathBuf::from("fixture-repository"));
    let packages = ConfigHandle::new(
        ["first", "second", "existing"]
            .into_iter()
            .map(|name| Package {
                name: name.into(),
                is_aur: false,
            })
            .collect(),
    );
    let mut mock = MockExecutor::new();
    mock.expect_which().return_const(true);
    let mut sequence = mockall::Sequence::new();
    mock.expect_execute()
        .once()
        .in_sequence(&mut sequence)
        .withf(is_native_inventory)
        .returning(|_| Ok(ExecResult::success("existing 1.0\n")));
    mock.expect_execute()
        .once()
        .in_sequence(&mut sequence)
        .withf(|spec| {
            spec.is_checked()
                && spec.arguments().ends_with(&[
                    std::ffi::OsString::from("first"),
                    std::ffi::OsString::from("second"),
                ])
        })
        .returning(|_| {
            Err(ExecError::non_zero(
                "pacman",
                ExecResult::failure("", "fixture second package failed", Some(1)),
            ))
        });
    mock.expect_execute()
        .once()
        .in_sequence(&mut sequence)
        .withf(is_native_inventory)
        .returning(|_| Ok(ExecResult::success("existing 1.0\nfirst 1.0\n")));
    let ctx = make_package_context(config, Os::Linux, true, mock);

    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    let stats = task_batch(&result);

    assert_eq!(
        (
            stats.changed_count(),
            stats.already_ok_count(),
            stats.failed_count()
        ),
        (1, 1, 1)
    );
}

#[test]
fn install_packages_propagates_batch_cancellation() {
    let root = tempfile::tempdir_in(".").unwrap();
    let mut config = empty_config(root.path().to_path_buf());
    config.packages.push(Package {
        name: "git".to_string(),
        is_aur: false,
    });
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    mock.expect_which().returning(|_| true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .withf(is_native_inventory)
        .returning(|_| Ok(ExecResult::success("")));
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .withf(is_native_git_install)
        .returning(|_| {
            Err(ExecError::Cancelled {
                command: "sudo pacman".to_string(),
                result: ExecResult::failure("", "", None),
            })
        });
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_package_context(config, Os::Linux, true, mock);

    let err = InstallPackages::new(packages)
        .run(&ctx)
        .expect_err("package cancellation must escape task accounting");

    assert!(
        err.downcast_ref::<ExecError>()
            .is_some_and(ExecError::is_cancelled)
    );
}

#[test]
fn install_packages_winget_installs_per_package() {
    let root = tempfile::tempdir_in(".").unwrap();
    let mut config = empty_config(root.path().to_path_buf());
    config.packages.push(Package {
        name: "Git.Git".to_string(),
        is_aur: false,
    });
    // which("winget") → true
    // run_unchecked("winget", ["list", ...]) → empty package table
    // run_unchecked("winget", ["install", ...]) → success
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    mock.expect_which().returning(|_| true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .withf(|spec| {
            spec.program() == "winget"
                && spec.arguments()
                    == [
                        "list",
                        "--accept-source-agreements",
                        "--disable-interactivity",
                    ]
                && !spec.is_checked()
                && spec.working_dir().is_none()
        })
        .returning(|_| Ok(ExecResult::success("Name  Id       Version\n")));
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .withf(|spec| {
            spec.program() == "winget"
                && spec.arguments()
                    == [
                        "install",
                        "--id",
                        "Git.Git",
                        "--exact",
                        "--source",
                        "winget",
                        "--accept-source-agreements",
                        "--accept-package-agreements",
                        "--disable-interactivity",
                        "--scope",
                        "user",
                    ]
                && !spec.is_checked()
                && spec.working_dir().is_none()
        })
        .returning(|_| Ok(ExecResult::success("")));
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_package_context(config, Os::Windows, false, mock);
    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    let stats = task_batch(&result);
    assert!(
        stats.changed_count() == 1 && stats.already_ok_count() == 0 && stats.failed_count() == 0,
        "expected changed package task result after winget per-package install, got {result:?}"
    );
}

#[test]
fn winget_already_current_after_discovery_is_not_an_install_failure() {
    let config = empty_config(PathBuf::from("fixture-repository"));
    let packages = ConfigHandle::new(vec![Package {
        name: "Git.Git".into(),
        is_aur: false,
    }]);
    let mut mock = MockExecutor::new();
    let mut sequence = mockall::Sequence::new();
    mock.expect_which()
        .once()
        .withf(|program| program == "winget")
        .return_const(true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut sequence)
        .withf(|spec| {
            spec.program() == "winget"
                && spec.arguments()
                    == [
                        "list",
                        "--accept-source-agreements",
                        "--disable-interactivity",
                    ]
                && !spec.is_checked()
        })
        .returning(|_| Ok(ExecResult::success("Name  Id       Version\n")));
    mock.expect_execute()
        .once()
        .in_sequence(&mut sequence)
        .withf(|spec| {
            spec.program() == "winget"
                && spec.arguments()
                    == [
                        "install",
                        "--id",
                        "Git.Git",
                        "--exact",
                        "--source",
                        "winget",
                        "--accept-source-agreements",
                        "--accept-package-agreements",
                        "--disable-interactivity",
                        "--scope",
                        "user",
                    ]
                && !spec.is_checked()
        })
        .returning(|_| {
            Ok(ExecResult::failure(
                "No available upgrade found.",
                "",
                Some(-1_978_335_189),
            ))
        });
    let ctx = make_package_context(config, Os::Windows, false, mock);

    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    let stats = task_batch(&result);

    assert_eq!(
        (
            stats.changed_count(),
            stats.already_ok_count(),
            stats.failed_count()
        ),
        (0, 1, 0),
    );
}

#[test]
fn winget_discovery_parse_failure_never_attempts_installation() {
    for dry_run in [false, true] {
        let config = empty_config(PathBuf::from("fixture-repository"));
        let packages = ConfigHandle::new(vec![Package {
            name: "Git.Git".to_string(),
            is_aur: false,
        }]);
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .once()
            .withf(|program| program == "winget")
            .returning(|_| true);
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "winget"
                    && spec
                        .arguments()
                        .first()
                        .is_some_and(|argument| argument == "list")
                    && !spec.is_checked()
            })
            .returning(|_| {
                Ok(ExecResult::success(
                    "Name  Identifier  Version\nGit   Git.Git     2.51.0\n",
                ))
            });
        let ctx = make_package_context(config, Os::Windows, false, mock).with_dry_run(dry_run);

        let error = InstallPackages::new(packages)
            .run(&ctx)
            .expect_err("unknown inventory must not plan every package for installation");

        assert!(error.to_string().contains("could not parse winget list"));
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
                        "other-package 1.0\n"
                    }))
                });
            let ctx = make_package_context(empty_config("/fixture".into()), os, arch, mock);
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
