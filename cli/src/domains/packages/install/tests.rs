//! Unit tests for package install tasks.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use crate::domains::packages::config::packages::Package;
use crate::domains::packages::test_fixture::{inventory, native_install, winget_install};
use crate::infra::ConfigHandle;
use crate::infra::exec::{CommandSpec, ExecError, ExecResult, MockExecutor};
use crate::infra::platform::Os;
use crate::test_helpers::{
    CapturingOutput, ScriptedExecutor, assert_task_changed, assert_task_ok,
    make_platform_context_with_which, task_batch, task_skipped,
};
use std::path::PathBuf;

fn packages(entries: &[(&str, bool)]) -> ConfigHandle<Vec<Package>> {
    ConfigHandle::new(
        entries
            .iter()
            .map(|&(name, is_aur)| Package {
                name: name.into(),
                is_aur,
            })
            .collect(),
    )
}

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

// -----------------------------------------------------------------------
// run() — early-exit paths that do not require a real package manager
// -----------------------------------------------------------------------

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

#[test]
fn installed_target_paru_is_accepted_when_host_path_lookup_would_miss() {
    let mut mock = MockExecutor::new();
    expect_installed_paru_package(&mut mock, 1);
    expect_healthy_paru(&mut mock, 1);
    let ctx = make_package_context(PathBuf::from("/tmp"), None, Os::Linux, true, mock);
    let result = InstallParu.run(&ctx).unwrap();
    assert_task_ok(&result);
}

#[test]
fn install_paru_run_returns_ok_when_already_installed_in_dry_run() {
    let mut mock = MockExecutor::new();
    expect_installed_paru_package(&mut mock, 1);
    expect_healthy_paru(&mut mock, 1);
    let mut ctx = make_package_context(PathBuf::from("/tmp"), None, Os::Linux, true, mock);
    ctx = ctx.with_dry_run(true);
    let result = InstallParu.run(&ctx).unwrap();
    assert_task_ok(&result);
}

#[test]
fn install_paru_run_returns_dry_run_when_not_installed_in_dry_run() {
    let mut mock = MockExecutor::new();
    expect_missing_paru_package(&mut mock, 1);
    expect_missing_paru_path(&mut mock, 1);
    let mut ctx = make_package_context(PathBuf::from("/tmp"), None, Os::Linux, true, mock);
    ctx = ctx.with_dry_run(true);
    let result = InstallParu.run(&ctx).unwrap();
    assert_task_changed(&result);
}

#[test]
fn paru_previews_keep_install_and_rebuild_verbs_separate_from_context() {
    for (plan, verb, subject) in [
        (
            ParuInstallPlan::Install {
                reason: "not installed".into(),
            },
            "install",
            "missing paru from AUR source",
        ),
        (
            ParuInstallPlan::Rebuild {
                path: PathBuf::from("/fixture/bin/paru"),
                reason: "missing shared library".into(),
            },
            "rebuild",
            "broken paru from AUR source",
        ),
    ] {
        let output = Arc::new(CapturingOutput::default());
        let ctx = make_package_context(
            PathBuf::from("/fixture"),
            None,
            Os::Linux,
            true,
            MockExecutor::new(),
        )
        .with_log(Arc::<CapturingOutput>::clone(&output))
        .with_dry_run(true);
        let operation = ParuInstallOperation {
            build_root: Path::new("/fixture"),
        };
        assert_task_changed(&operation.preview(&ctx, &plan).unwrap());
        let actions = output.actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].verb, verb);
        assert_eq!(actions[0].subject, subject);
        assert!(actions[0].planned);
        assert!(
            actions[0]
                .message
                .starts_with(&format!("{verb} {subject} · "))
        );
    }
}

#[test]
fn install_paru_reports_change_and_cleans_its_fixture_build_directory() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let legacy = fixture.path().join("paru-build");
    std::fs::create_dir(&legacy).unwrap();
    std::fs::write(legacy.join("keep"), "existing user checkout").unwrap();
    let unrelated = fixture.path().join("unrelated");
    std::fs::create_dir(&unrelated).unwrap();
    std::fs::write(unrelated.join("keep"), "unrelated content").unwrap();

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
    let build_dir = expect_paru_clone(&mut mock, fixture.path());
    mock.expect_execute()
        .once()
        .withf(is_makepkg(Arc::clone(&build_dir)))
        .returning(|_| Ok(ExecResult::success("")));
    expect_healthy_paru(&mut mock, 1);

    let output = Arc::new(CapturingOutput::default());
    let ctx = make_package_context(fixture.path().to_path_buf(), None, Os::Linux, true, mock)
        .with_log(Arc::<CapturingOutput>::clone(&output));
    let result = run_paru_install(&ctx, fixture.path()).unwrap();
    let stats = task_batch(&result);
    assert!(
        stats.changed_count() > 0 && stats.message() == Some("installed paru"),
        "expected changed result after paru install, got {result:?}"
    );
    let actions = output.actions();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].verb, "install");
    assert_eq!(actions[0].subject, "paru");
    assert!(!actions[0].planned);
    assert!(
        actions[0]
            .message
            .starts_with("install paru · target validated · ")
    );
    assert!(
        !build_dir.get().unwrap().exists(),
        "successful installation must clean its checkout"
    );
    assert_eq!(
        std::fs::read_to_string(legacy.join("keep")).unwrap(),
        "existing user checkout",
        "a predictable old build directory is not owned by this invocation"
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

        let mut mock = MockExecutor::new();
        if installed {
            expect_installed_paru_package(&mut mock, 1);
            expect_healthy_paru(&mut mock, 1);
        } else {
            expect_missing_paru_package(&mut mock, 1);
            expect_missing_paru_path(&mut mock, 1);
        }
        let ctx = make_package_context(fixture.path().to_path_buf(), None, Os::Linux, true, mock)
            .with_dry_run(dry_run);

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
    let build_dir = Arc::new(OnceLock::<PathBuf>::new());
    let marker = fixture.path().join("keep");
    std::fs::write(&marker, "unrelated content").unwrap();

    let mut mock = MockExecutor::new();
    expect_missing_paru_package(&mut mock, 1);
    expect_missing_paru_path(&mut mock, 1);
    expect_paru_build_prerequisites(&mut mock);
    let partial = Arc::clone(&build_dir);
    mock.expect_execute()
        .once()
        .withf(is_git_clone(fixture.path().into()))
        .returning(move |spec| {
            let path = PathBuf::from(spec.arguments().last().unwrap());
            assert!(
                path.is_dir(),
                "the build directory must be reserved before cloning"
            );
            std::fs::write(path.join("partial"), "incomplete checkout").unwrap();
            partial.set(path).unwrap();
            Err(ExecError::non_zero(
                "git clone",
                ExecResult::failure("", "fixture clone failed", Some(128)),
            ))
        });
    let ctx = make_package_context(fixture.path().to_path_buf(), None, Os::Linux, true, mock);

    let error = run_paru_install(&ctx, fixture.path()).unwrap_err();

    assert!(
        format!("{error:#}").contains("fixture clone failed"),
        "{error:#}"
    );
    assert!(
        !build_dir.get().unwrap().exists(),
        "failed clone must clean its partial checkout"
    );
    assert_eq!(
        std::fs::read_to_string(marker).unwrap(),
        "unrelated content"
    );
}

#[test]
fn install_aur_packages_errors_when_paru_disappears_after_bootstrap() {
    let packages = packages(&[("paru-bin", true)]);
    let mut mock = MockExecutor::new();
    expect_missing_paru_package(&mut mock, 1);
    expect_missing_paru_path(&mut mock, 1);
    let ctx = make_package_context(PathBuf::from("/tmp"), None, Os::Linux, true, mock);
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

fn is_git_clone(build_root: PathBuf) -> impl Fn(&CommandSpec) -> bool {
    move |spec| {
        spec.program() == "git"
            && matches!(spec.arguments(), [command, url, destination]
                if command == "clone"
                    && url == "https://aur.archlinux.org/paru.git"
                    && Path::new(destination).parent() == Some(build_root.as_path())
                    && Path::new(destination).file_name().unwrap().to_str().unwrap().starts_with("paru-build-"))
            && spec.is_checked()
            && spec.working_dir().is_none()
    }
}

fn expect_paru_clone(mock: &mut MockExecutor, build_root: &Path) -> Arc<OnceLock<PathBuf>> {
    let build_dir = Arc::new(OnceLock::new());
    let captured = Arc::clone(&build_dir);
    mock.expect_execute()
        .once()
        .withf(is_git_clone(build_root.into()))
        .returning(move |spec| {
            let path = PathBuf::from(spec.arguments().last().unwrap());
            assert_eq!(
                std::fs::read_dir(&path).unwrap().count(),
                0,
                "clone must receive an exclusively reserved empty directory"
            );
            std::fs::write(path.join("PKGBUILD"), "fixture checkout").unwrap();
            captured.set(path).unwrap();
            Ok(ExecResult::success(""))
        });
    build_dir
}

fn is_cargo_version(spec: &CommandSpec) -> bool {
    spec.program() == cargo_path().as_os_str()
        && spec.arguments() == ["--version"]
        && spec.is_checked()
        && spec.working_dir().is_none()
}

fn is_makepkg(build_dir: Arc<OnceLock<PathBuf>>) -> impl Fn(&CommandSpec) -> bool {
    move |spec| {
        spec.program() == "makepkg"
            && spec.arguments() == ["-si", "--noconfirm"]
            && spec.is_checked()
            && spec.working_dir() == build_dir.get().map(PathBuf::as_path)
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
    let mut mock = MockExecutor::new();
    expect_paru_native_build_tools(&mut mock);
    mock.expect_which_path()
        .once()
        .with(mockall::predicate::eq("cargo"))
        .returning(|_| anyhow::bail!("cargo not found on PATH"));

    let ctx = make_package_context(PathBuf::from("/tmp"), None, Os::Linux, true, mock);
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
                Err(ExecError::non_zero(
                    "pacman -Q",
                    ExecResult::failure("", "fixture unreadable package database", Some(1)),
                ))
            });

        let ctx = make_package_context(
            PathBuf::from("fixture-repository"),
            None,
            Os::Linux,
            true,
            mock,
        )
        .with_dry_run(dry_run);

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

                let ctx = make_package_context(
                    PathBuf::from("fixture-repository"),
                    None,
                    Os::Linux,
                    true,
                    mock,
                )
                .with_dry_run(dry_run);

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

    let ctx = make_package_context(
        PathBuf::from("fixture-repository"),
        None,
        Os::Linux,
        true,
        mock,
    );

    assert!(!InstallParu.needs_elevation(&ctx));
}

#[test]
fn paru_prerequisites_reject_unconfigured_rustup_cargo() {
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

    let ctx = make_package_context(PathBuf::from("/tmp"), None, Os::Linux, true, mock);
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
    let build_dir = expect_paru_clone(&mut mock, fixture.path());
    mock.expect_execute()
        .once()
        .withf(is_makepkg(Arc::clone(&build_dir)))
        .returning(|_| Ok(ExecResult::success("")));

    let output = Arc::new(CapturingOutput::default());
    let ctx = make_package_context(fixture.path().to_path_buf(), None, Os::Linux, true, mock)
        .with_log(Arc::<CapturingOutput>::clone(&output));
    let result = run_paru_install(&ctx, fixture.path()).unwrap();
    let stats = task_batch(&result);

    assert_eq!(stats.message(), Some("rebuilt paru"));
    let actions = output.actions();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].verb, "rebuild");
    assert_eq!(actions[0].subject, "paru");
    assert!(!actions[0].planned);
    assert!(
        actions[0]
            .message
            .starts_with("rebuild paru · target validated · ")
    );
    assert!(
        !build_dir.get().unwrap().exists(),
        "successful rebuild must clean its checkout"
    );
}

#[test]
fn failed_paru_rebuild_returns_clear_root_error() {
    let fixture = tempfile::tempdir_in(".").unwrap();

    let mut mock = MockExecutor::new();
    let build_dir = expect_failed_paru_rebuild(&mut mock, fixture.path());

    let ctx = make_package_context(fixture.path().to_path_buf(), None, Os::Linux, true, mock);
    let error = run_paru_install(&ctx, fixture.path()).unwrap_err();
    let message = format!("{error:#}");

    assert!(message.contains("paru rebuild attempt failed"), "{message}");
    assert!(message.contains("package build failed"), "{message}");
    assert!(
        !build_dir.get().unwrap().exists(),
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
    let packages = packages(&[("apm-bin", true)]);
    let mut mock = MockExecutor::new();
    let build_dir = expect_failed_paru_rebuild(&mut mock, fixture.path());

    let (log, _tmp, _guard) = crate::infra::logging::isolated_logger();
    let log = Arc::new(log);
    let log_output: Arc<dyn Log> = Arc::<crate::infra::logging::Logger>::clone(&log);
    let ctx = make_package_context(fixture.path().to_path_buf(), None, Os::Linux, true, mock)
        .with_log(log_output);
    let install_paru = FixtureParuTask {
        inner: InstallParu,
        build_dir: fixture.path().into(),
    };
    let install_aur = InstallAurPackages::new(packages);
    let tasks: Vec<&dyn Task> = vec![&install_paru, &install_aur];
    let graph = ResolvedTaskGraph::resolve(&tasks).unwrap();
    let assessments = tasks
        .iter()
        .map(|task| (task.task_id(), TaskAssessment::applicable()))
        .collect::<HashMap<_, _>>();

    let summary = crate::engine::scheduler::run_tasks(
        &tasks,
        &graph,
        &assessments,
        &ctx.with_parallel(false),
        &log,
        None,
    );

    assert_eq!(summary.failure_count(), 1);
    assert!(
        !build_dir.get().unwrap().exists(),
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

fn expect_failed_paru_rebuild(
    mock: &mut MockExecutor,
    build_root: &Path,
) -> Arc<OnceLock<PathBuf>> {
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
    let build_dir = expect_paru_clone(mock, build_root);
    mock.expect_execute()
        .once()
        .withf(is_makepkg(Arc::clone(&build_dir)))
        .returning(|_| {
            Err(ExecError::non_zero(
                "makepkg -si --noconfirm",
                ExecResult::failure("", "package build failed", Some(4)),
            ))
        });
    build_dir
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

fn make_package_context(
    root: PathBuf,
    overlay: Option<PathBuf>,
    os: Os,
    is_arch: bool,
    executor: impl crate::infra::exec::Executor + 'static,
) -> Context {
    use crate::infra::platform::Platform;
    crate::test_helpers::make_context(
        root,
        overlay,
        Platform::new(os, is_arch),
        Arc::new(executor),
    )
}

fn is_native_inventory(spec: &CommandSpec) -> bool {
    *spec == inventory()
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

#[test]
fn cancelled_winget_task_preserves_completed_and_initially_current_counts() {
    let packages = packages(&[
        ("First.App", false),
        ("Second.App", false),
        ("Third.App", false),
        ("Existing.App", false),
    ]);
    let mut mock = MockExecutor::new();
    mock.expect_which().return_const(true);
    let mut sequence = mockall::Sequence::new();
    mock.expect_execute()
        .once()
        .in_sequence(&mut sequence)
        .returning(|spec| {
            crate::domains::packages::resources::mock_inventory(&spec, &["Existing.App"]);
            Ok(ExecResult::success(""))
        });
    mock.expect_execute()
        .once()
        .in_sequence(&mut sequence)
        .with(mockall::predicate::eq(winget_install("First.App")))
        .returning(|_| Ok(ExecResult::success("installed")));
    mock.expect_execute()
        .once()
        .in_sequence(&mut sequence)
        .with(mockall::predicate::eq(winget_install("Second.App")))
        .returning(|_| {
            Err(ExecError::Cancelled {
                command: "winget install".into(),
                result: ExecResult::failure("", "", None),
            })
        });
    let ctx = make_package_context(
        PathBuf::from("fixture-repository"),
        None,
        Os::Windows,
        false,
        mock,
    );

    let error = InstallPackages::new(packages).run(&ctx).unwrap_err();
    let report = error.downcast_ref::<BatchReport>().unwrap();
    assert_eq!(report.completion(), BatchCompletion::Interrupted);
    assert_eq!(report.stats().changed_count(), 1);
    assert_eq!(report.stats().already_ok_count(), 1);
    assert_eq!(report.stats().failed_count(), 0);
    assert_eq!(report.interrupted_count(), 1);
    assert_eq!(report.not_attempted_count(), 1);
    assert!(error.downcast_ref::<ExecError>().unwrap().is_cancelled());
}

#[test]
fn winget_install_matches_ascii_case_and_installs_only_missing_configured_ids() {
    for (configured_git, install_result, expected) in [
        ("git.git", ExecResult::success(""), (1, 1, 0)),
        (
            "gIt.GIT",
            ExecResult::failure("No available upgrade found.", "", Some(-1_978_335_189)),
            (0, 2, 0),
        ),
    ] {
        let mut mock = MockExecutor::new();
        let mut sequence = mockall::Sequence::new();
        mock.expect_which()
            .once()
            .with(mockall::predicate::eq("winget"))
            .return_const(true);
        mock.expect_execute()
            .once()
            .in_sequence(&mut sequence)
            .returning(|spec| {
                crate::domains::packages::resources::mock_inventory(
                    &spec,
                    &["Git.Git", "Äpp.Tool"],
                );
                Ok(ExecResult::success(""))
            });
        mock.expect_execute()
            .once()
            .in_sequence(&mut sequence)
            .with(mockall::predicate::eq(winget_install("äpp.ToOl")))
            .return_once(|_| Ok(install_result));
        let ctx = make_package_context("fixture-repository".into(), None, Os::Windows, false, mock);
        // ASCII case changes don't make Git missing; ä and Ä are distinct.
        let entries = packages(&[(configured_git, false), ("äpp.ToOl", false)]);
        let result = InstallPackages::new(entries).run(&ctx).unwrap();
        let stats = task_batch(&result);
        assert_eq!(
            (
                stats.changed_count(),
                stats.already_ok_count(),
                stats.failed_count()
            ),
            expected
        );
    }
}

#[test]
fn winget_discovery_parse_failure_never_attempts_installation() {
    for dry_run in [false, true] {
        let packages = packages(&[("Git.Git", false)]);
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
                        .is_some_and(|argument| argument == "export")
                    && spec.is_checked()
            })
            .returning(|spec| {
                crate::domains::packages::resources::mock_inventory(&spec, &[]);
                std::fs::write(&spec.arguments()[2], r#"{"Sources":[{}]}"#).unwrap();
                Ok(ExecResult::success(""))
            });
        let ctx = make_package_context(
            PathBuf::from("fixture-repository"),
            None,
            Os::Windows,
            false,
            mock,
        )
        .with_dry_run(dry_run);

        let error = InstallPackages::new(packages)
            .run(&ctx)
            .expect_err("unknown inventory must not plan every package for installation");

        assert!(error.to_string().contains("could not parse winget export"));
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
