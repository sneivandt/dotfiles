//! Unit tests for package install tasks.

use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::domains::packages::config::packages::Package;
use crate::domains::packages::resources::package::{PackageManager, PackageResource};
use crate::engine::Resource;
use crate::infra::ConfigHandle;
use crate::infra::exec::{ExecError, ExecResult, Executor, MockExecutor};
use crate::infra::platform::Os;
use crate::test_helpers::{
    assert_task_changed, assert_task_ok, empty_config, make_arch_context, make_linux_context,
    make_platform_context_with_which, make_windows_context, task_batch, task_skipped,
};
use std::path::PathBuf;

#[test]
fn package_resource_description() {
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

// -----------------------------------------------------------------------
// InstallPackages::should_run
// -----------------------------------------------------------------------

#[test]
fn install_packages_should_run_false_when_no_packages() {
    let config = empty_config(PathBuf::from("/tmp"));
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_linux_context(config);
    assert!(!InstallPackages::new(packages).should_run(&ctx));
}

#[test]
fn install_packages_should_run_false_when_only_aur_packages() {
    let mut config = empty_config(PathBuf::from("/tmp"));
    config.packages.push(Package {
        name: "paru-bin".to_string(),
        is_aur: true,
    });
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_arch_context(config);
    assert!(!InstallPackages::new(packages).should_run(&ctx));
}

#[test]
fn install_packages_should_run_true_when_non_aur_packages_present() {
    let mut config = empty_config(PathBuf::from("/tmp"));
    config.packages.push(Package {
        name: "git".to_string(),
        is_aur: false,
    });
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_linux_context(config);
    assert!(InstallPackages::new(packages).should_run(&ctx));
}

// -----------------------------------------------------------------------
// InstallAurPackages::should_run
// -----------------------------------------------------------------------

#[test]
fn install_aur_packages_should_run_false_on_non_arch() {
    let mut config = empty_config(PathBuf::from("/tmp"));
    config.packages.push(Package {
        name: "paru-bin".to_string(),
        is_aur: true,
    });
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_linux_context(config); // not arch
    assert!(!InstallAurPackages::new(packages).should_run(&ctx));
}

#[test]
fn install_aur_packages_should_run_false_when_no_aur_packages() {
    let mut config = empty_config(PathBuf::from("/tmp"));
    config.packages.push(Package {
        name: "git".to_string(),
        is_aur: false,
    });
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_arch_context(config);
    assert!(!InstallAurPackages::new(packages).should_run(&ctx));
}

#[test]
fn install_aur_packages_should_run_true_on_arch_with_aur_packages() {
    let mut config = empty_config(PathBuf::from("/tmp"));
    config.packages.push(Package {
        name: "paru-bin".to_string(),
        is_aur: true,
    });
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_arch_context(config);
    assert!(InstallAurPackages::new(packages).should_run(&ctx));
}

// -----------------------------------------------------------------------
// InstallParu::should_run
// -----------------------------------------------------------------------

#[test]
fn install_paru_should_run_false_on_non_arch_linux() {
    let config = empty_config(PathBuf::from("/tmp"));
    let ctx = make_linux_context(config);
    assert!(!InstallParu.should_run(&ctx));
}

#[test]
fn install_paru_should_run_false_on_windows() {
    let config = empty_config(PathBuf::from("/tmp"));
    let ctx = make_windows_context(config);
    assert!(!InstallParu.should_run(&ctx));
}

#[test]
fn install_paru_should_run_true_on_arch_linux() {
    let config = empty_config(PathBuf::from("/tmp"));
    let ctx = make_arch_context(config);
    assert!(InstallParu.should_run(&ctx));
}

// -----------------------------------------------------------------------
// run() — early-exit paths that do not require a real package manager
// -----------------------------------------------------------------------

#[test]
fn install_packages_run_skips_when_pacman_not_found() {
    let mut config = empty_config(PathBuf::from("/tmp"));
    config.packages.push(Package {
        name: "git".to_string(),
        is_aur: false,
    });
    // which_result=false ⇒ pacman not found
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_platform_context_with_which(config, Os::Linux, false, false);
    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    let reason = task_skipped(&result);
    assert!(
        reason.contains("pacman not found"),
        "expected 'pacman not found' skip, got {reason:?}"
    );
}

#[test]
fn install_packages_run_skips_when_winget_not_found() {
    let mut config = empty_config(PathBuf::from("/tmp"));
    config.packages.push(Package {
        name: "Git.Git".to_string(),
        is_aur: false,
    });
    // which_result=false ⇒ winget not found
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_platform_context_with_which(config, Os::Windows, false, false);
    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    let reason = task_skipped(&result);
    assert!(
        reason.contains("winget not found"),
        "expected 'winget not found' skip, got {reason:?}"
    );
}

#[test]
fn install_paru_run_returns_ok_when_already_installed() {
    let config = empty_config(PathBuf::from("/tmp"));
    let mut mock = MockExecutor::new();
    expect_paru_path(&mut mock, 1);
    expect_healthy_paru(&mut mock, 1);
    let ctx = make_package_context(config, Os::Linux, true, mock);
    let result = InstallParu.run(&ctx).unwrap();
    assert_task_ok(&result);
}

#[test]
fn install_paru_run_returns_ok_when_already_installed_in_dry_run() {
    let config = empty_config(PathBuf::from("/tmp"));
    let mut mock = MockExecutor::new();
    expect_paru_path(&mut mock, 1);
    expect_healthy_paru(&mut mock, 1);
    let mut ctx = make_package_context(config, Os::Linux, true, mock);
    ctx = ctx.with_dry_run(true);
    let result = InstallParu.run(&ctx).unwrap();
    assert_task_ok(&result);
}

#[test]
fn install_paru_run_returns_dry_run_when_not_installed_in_dry_run() {
    let config = empty_config(PathBuf::from("/tmp"));
    // which_result=false ⇒ paru missing in PATH
    let mut ctx = make_platform_context_with_which(config, Os::Linux, true, false);
    ctx = ctx.with_dry_run(true);
    let result = InstallParu.run(&ctx).unwrap();
    assert_task_changed(&result);
}

#[test]
fn install_paru_run_returns_changed_result_after_install() {
    let config = empty_config(PathBuf::from("/tmp"));
    let mut mock = MockExecutor::new();
    let lookups = Arc::new(AtomicUsize::new(0));
    mock.expect_which_path()
        .times(2)
        .with(mockall::predicate::eq("paru"))
        .returning(move |_| {
            if lookups.fetch_add(1, Ordering::SeqCst) == 0 {
                anyhow::bail!("paru not found on PATH")
            }
            Ok(paru_path())
        });
    for dep in ["git", "makepkg", "sudo"] {
        mock.expect_which()
            .once()
            .with(mockall::predicate::eq(dep))
            .returning(|_| true);
    }
    mock.expect_execute()
        .once()
        .withf(is_git_clone)
        .returning(|spec| {
            assert_eq!(spec.program(), "git");
            assert_eq!(spec.arguments()[0], "clone");
            assert_eq!(spec.arguments()[1], "https://aur.archlinux.org/paru.git");
            Ok(ExecResult::success(""))
        });
    mock.expect_execute()
        .once()
        .withf(is_makepkg)
        .returning(|spec| {
            assert_eq!(spec.arguments(), ["-si", "--noconfirm"]);
            assert_eq!(spec.environment().len(), 1);
            assert_eq!(spec.environment()[0].0, "MAKEFLAGS");
            Ok(ExecResult::success(""))
        });
    expect_healthy_paru(&mut mock, 1);

    let ctx = make_package_context(config, Os::Linux, true, mock);
    let result = InstallParu.run(&ctx).unwrap();
    let stats = task_batch(&result);
    assert!(
        stats.changed_count() > 0 && stats.message() == Some("installed paru"),
        "expected changed result after paru install, got {result:?}"
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
    let ctx = make_platform_context_with_which(config, Os::Linux, true, false);
    let error = InstallAurPackages::new(packages).run(&ctx).unwrap_err();
    assert!(
        error.to_string().contains("became unavailable"),
        "expected explicit post-bootstrap health error, got {error:#}"
    );
}

fn paru_path() -> PathBuf {
    PathBuf::from("/usr/bin/paru")
}

fn is_paru_version(spec: &crate::infra::exec::CommandSpec) -> bool {
    spec.program() == paru_path().as_os_str() && spec.arguments() == ["--version"]
}

fn is_git_clone(spec: &crate::infra::exec::CommandSpec) -> bool {
    spec.program() == "git" && spec.arguments().first().is_some_and(|arg| arg == "clone")
}

fn is_makepkg(spec: &crate::infra::exec::CommandSpec) -> bool {
    spec.program() == "makepkg"
}

fn expect_paru_path(mock: &mut MockExecutor, times: usize) {
    mock.expect_which_path()
        .times(times)
        .with(mockall::predicate::eq("paru"))
        .returning(|_| Ok(paru_path()));
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
    expect_paru_path(&mut mock, 1);
    mock.expect_execute()
        .once()
        .withf(is_paru_version)
        .returning(|_| {
            Err(ExecError::non_zero(
                "/usr/bin/paru --version",
                ExecResult::failure("", "unexpected failure", Some(1)),
            ))
        });

    let health = check_paru_health(&mock);

    assert!(matches!(
        health,
        ParuHealth::Broken { path, reason }
            if path == paru_path() && reason.contains("unexpected failure")
    ));
}

#[test]
fn paru_health_preserves_missing_libalpm_failure() {
    let mut mock = MockExecutor::new();
    expect_paru_path(&mut mock, 1);
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

    let health = check_paru_health(&mock);

    assert!(matches!(
        health,
        ParuHealth::Broken { path, reason }
            if path == paru_path() && reason.contains("libalpm.so.15")
    ));
}

#[test]
fn broken_paru_is_rebuilt_and_revalidated() {
    let config = empty_config(PathBuf::from("/tmp"));
    let mut mock = MockExecutor::new();
    expect_paru_path(&mut mock, 2);
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
    mock.expect_execute()
        .once()
        .withf(is_git_clone)
        .returning(|_| Ok(ExecResult::success("")));
    mock.expect_execute()
        .once()
        .withf(is_makepkg)
        .returning(|_| Ok(ExecResult::success("")));

    let ctx = make_package_context(config, Os::Linux, true, mock);
    let result = InstallParu.run(&ctx).unwrap();
    let stats = task_batch(&result);

    assert_eq!(stats.message(), Some("rebuilt paru"));
}

#[test]
fn failed_paru_rebuild_returns_clear_root_error() {
    let config = empty_config(PathBuf::from("/tmp"));
    let mut mock = MockExecutor::new();
    expect_paru_path(&mut mock, 1);
    mock.expect_execute()
        .once()
        .withf(is_paru_version)
        .returning(|_| {
            Err(ExecError::non_zero(
                "/usr/bin/paru --version",
                ExecResult::failure("", "libalpm.so.15 not found", Some(127)),
            ))
        });
    expect_paru_build_prerequisites(&mut mock);
    mock.expect_execute()
        .once()
        .withf(is_git_clone)
        .returning(|_| Ok(ExecResult::success("")));
    mock.expect_execute()
        .once()
        .withf(is_makepkg)
        .returning(|_| {
            Err(ExecError::non_zero(
                "makepkg -si --noconfirm",
                ExecResult::failure("", "package build failed", Some(4)),
            ))
        });

    let ctx = make_package_context(config, Os::Linux, true, mock);
    let error = InstallParu.run(&ctx).unwrap_err();
    let message = format!("{error:#}");

    assert!(message.contains("paru rebuild attempt failed"), "{message}");
    assert!(message.contains("package build failed"), "{message}");
}

#[test]
fn failed_paru_rebuild_blocks_aur_package_task() {
    use std::collections::HashMap;

    use crate::engine::TaskAssessment;
    use crate::engine::graph::ResolvedTaskGraph;
    use crate::infra::logging::{Log, TaskStatus};

    let mut config = empty_config(PathBuf::from("/tmp"));
    config.packages.push(Package {
        name: "apm-bin".to_string(),
        is_aur: true,
    });
    let packages = ConfigHandle::new(config.packages.clone());
    let mut mock = MockExecutor::new();
    expect_paru_path(&mut mock, 1);
    mock.expect_execute()
        .once()
        .withf(is_paru_version)
        .returning(|_| {
            Err(ExecError::non_zero(
                "/usr/bin/paru --version",
                ExecResult::failure("", "libalpm.so.15 not found", Some(127)),
            ))
        });
    expect_paru_build_prerequisites(&mut mock);
    mock.expect_execute()
        .once()
        .withf(is_git_clone)
        .returning(|_| Ok(ExecResult::success("")));
    mock.expect_execute()
        .once()
        .withf(is_makepkg)
        .returning(|_| {
            Err(ExecError::non_zero(
                "makepkg -si --noconfirm",
                ExecResult::failure("", "package build failed", Some(4)),
            ))
        });

    let (log, _tmp, _guard) = crate::infra::logging::isolated_logger();
    let log = Arc::new(log);
    let log_output: Arc<dyn Log> = Arc::<crate::infra::logging::Logger>::clone(&log);
    let ctx = make_package_context(config, Os::Linux, true, mock).with_log(log_output);
    let install_paru = InstallParu;
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
    assert_eq!(aur_entry.status, TaskStatus::Skipped);
    assert_eq!(aur_entry.message.as_deref(), Some("dependency failed"));

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

fn expect_paru_build_prerequisites(mock: &mut MockExecutor) {
    for dependency in ["git", "makepkg", "sudo"] {
        mock.expect_which()
            .once()
            .with(mockall::predicate::eq(dependency))
            .returning(|_| true);
    }
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

#[test]
fn install_packages_batch_installs_missing_packages_on_arch() {
    let mut config = empty_config(PathBuf::from("/tmp"));
    config.packages.push(Package {
        name: "git".to_string(),
        is_aur: false,
    });
    config.packages.push(Package {
        name: "vim".to_string(),
        is_aur: false,
    });
    // which("pacman") → true
    // run_unchecked("pacman", ["-Q"]) → vim installed, git not
    // run("sudo", ["pacman", "-S", "--needed", "--noconfirm", "git"]) → success
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    mock.expect_which().returning(|_| true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|_| Ok(ExecResult::success("vim 9.0\n")));
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
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
    let mut config = empty_config(PathBuf::from("/tmp"));
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
        .returning(|_| Ok(ExecResult::success("git 2.40\n")));
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_package_context(config, Os::Linux, false, mock);
    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    assert_task_ok(&result);
}

#[test]
fn install_packages_dry_run_reports_missing_packages() {
    let mut config = empty_config(PathBuf::from("/tmp"));
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
        .returning(|_| Ok(ExecResult::success("")));
    let packages = ConfigHandle::new(config.packages.clone());
    let mut ctx = make_package_context(config, Os::Linux, true, mock);
    ctx = ctx.with_dry_run(true);
    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    assert_eq!(task_batch(&result).changed_count(), 1);
}

#[test]
fn install_packages_returns_failed_when_batch_install_fails() {
    let mut config = empty_config(PathBuf::from("/tmp"));
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
        .returning(|_| Ok(ExecResult::success("")));
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|_| {
            Err(ExecError::spawn(
                "sudo",
                std::io::Error::other("failed (exit 1)"),
            ))
        });
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_package_context(config, Os::Linux, true, mock);
    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    assert_eq!(task_batch(&result).failed_count(), 1);
}

#[test]
fn install_packages_propagates_batch_cancellation() {
    let mut config = empty_config(PathBuf::from("/tmp"));
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
        .returning(|_| Ok(ExecResult::success("")));
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
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
    let mut config = empty_config(PathBuf::from("/tmp"));
    config.packages.push(Package {
        name: "Git.Git".to_string(),
        is_aur: false,
    });
    // which("winget") → true
    // run_unchecked("winget", ["list", ...]) → empty (nothing installed)
    // run_unchecked("winget", ["install", ...]) → success
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    mock.expect_which().returning(|_| true);
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .returning(|_| Ok(ExecResult::success("")));
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
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
