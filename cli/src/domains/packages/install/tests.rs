//! Unit tests for package install tasks.

use super::*;
use std::sync::Arc;

use crate::domains::packages::config::packages::Package;
use crate::domains::packages::resources::package::{PackageManager, PackageResource};
use crate::engine::Resource;
use crate::infra::ConfigHandle;
use crate::infra::exec::{ExecResult, Executor, MockExecutor};
use crate::infra::platform::Os;
use crate::test_helpers::{
    empty_config, make_arch_context, make_linux_context, make_platform_context_with_which,
    make_windows_context,
};
use std::path::PathBuf;

#[test]
fn package_resource_description() {
    let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::SystemExecutor);
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
    assert!(
        matches!(result, TaskResult::Skipped(ref s) if s.contains("pacman not found")),
        "expected 'pacman not found' skip, got {result:?}"
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
    assert!(
        matches!(result, TaskResult::Skipped(ref s) if s.contains("winget not found")),
        "expected 'winget not found' skip, got {result:?}"
    );
}

#[test]
fn install_paru_run_returns_ok_when_already_installed() {
    let config = empty_config(PathBuf::from("/tmp"));
    // which_result=true ⇒ paru found in PATH
    let ctx = make_platform_context_with_which(config, Os::Linux, true, true);
    let result = InstallParu.run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok when paru already installed, got {result:?}"
    );
}

#[test]
fn install_paru_run_returns_ok_when_already_installed_in_dry_run() {
    let config = empty_config(PathBuf::from("/tmp"));
    // which_result=true ⇒ paru found in PATH
    let mut ctx = make_platform_context_with_which(config, Os::Linux, true, true);
    ctx = ctx.with_dry_run(true);
    let result = InstallParu.run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok when paru already installed in dry-run mode, got {result:?}"
    );
}

#[test]
fn install_paru_run_returns_dry_run_when_not_installed_in_dry_run() {
    let config = empty_config(PathBuf::from("/tmp"));
    // which_result=false ⇒ paru missing in PATH
    let mut ctx = make_platform_context_with_which(config, Os::Linux, true, false);
    ctx = ctx.with_dry_run(true);
    let result = InstallParu.run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::DryRun),
        "expected DryRun when paru missing in dry-run mode, got {result:?}"
    );
}

#[test]
fn install_paru_run_returns_changed_result_after_install() {
    let config = empty_config(PathBuf::from("/tmp"));
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    mock.expect_which()
        .once()
        .with(mockall::predicate::eq("paru"))
        .returning(|_| false);
    for dep in ["git", "makepkg", "sudo"] {
        mock.expect_which()
            .once()
            .with(mockall::predicate::eq(dep))
            .returning(|_| true);
    }
    mock.expect_run()
        .once()
        .in_sequence(&mut seq)
        .returning(|program, args| {
            assert_eq!(program, "git");
            assert_eq!(args[0], "clone");
            Ok(ok_result(""))
        });
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, program, args, env| {
            assert_eq!(program, "makepkg");
            assert_eq!(args, ["-si", "--noconfirm"]);
            assert_eq!(env.len(), 1);
            assert_eq!(env[0].0, "MAKEFLAGS");
            Ok(ok_result(""))
        });

    let ctx = make_package_context(config, Os::Linux, true, mock);
    let result = InstallParu.run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::OkWithMessage(ref message) if message == "installed paru"),
        "expected changed result after paru install, got {result:?}"
    );
}

#[test]
fn install_aur_packages_run_skips_when_paru_not_found() {
    let mut config = empty_config(PathBuf::from("/tmp"));
    config.packages.push(Package {
        name: "paru-bin".to_string(),
        is_aur: true,
    });
    // which_result=false ⇒ paru not found
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_platform_context_with_which(config, Os::Linux, true, false);
    let result = InstallAurPackages::new(packages).run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Skipped(ref s) if s.contains("paru not installed")),
        "expected 'paru not installed' skip, got {result:?}"
    );
}

// -----------------------------------------------------------------------
// run() — batch install paths (pacman/paru)
// -----------------------------------------------------------------------

fn ok_result(stdout: &str) -> ExecResult {
    ExecResult {
        stdout: stdout.to_string(),
        stderr: String::new(),
        success: true,
        code: Some(0),
    }
}

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
    mock.expect_run_unchecked()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _| Ok(ok_result("vim 9.0\n")));
    mock.expect_run()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _| Ok(ok_result("")));
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_package_context(config, Os::Linux, true, mock);
    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    assert!(
        matches!(
            result,
            TaskResult::Batch(ref stats)
                if stats.changed == 1 && stats.already_ok == 1 && stats.failed == 0
        ),
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
    mock.expect_run_unchecked()
        .once()
        .returning(|_, _| Ok(ok_result("git 2.40\n")));
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_package_context(config, Os::Linux, false, mock);
    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok when all packages are installed, got {result:?}"
    );
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
    mock.expect_run_unchecked()
        .once()
        .returning(|_, _| Ok(ok_result("")));
    let packages = ConfigHandle::new(config.packages.clone());
    let mut ctx = make_package_context(config, Os::Linux, true, mock);
    ctx = ctx.with_dry_run(true);
    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Batch(ref stats) if stats.changed == 1),
        "expected one planned package action, got {result:?}"
    );
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
    mock.expect_run_unchecked()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _| Ok(ok_result("")));
    mock.expect_run()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _| Err(anyhow::anyhow!("sudo failed (exit 1)")));
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_package_context(config, Os::Linux, true, mock);
    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Batch(ref stats) if stats.failed == 1),
        "expected one failed package action, got {result:?}"
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
    mock.expect_run_unchecked()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _| Ok(ok_result("")));
    mock.expect_run_unchecked()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _| Ok(ok_result("")));
    let packages = ConfigHandle::new(config.packages.clone());
    let ctx = make_package_context(config, Os::Windows, false, mock);
    let result = InstallPackages::new(packages).run(&ctx).unwrap();
    assert!(
        matches!(
            result,
            TaskResult::Batch(ref stats)
                if stats.changed == 1 && stats.already_ok == 0 && stats.failed == 0
        ),
        "expected changed package task result after winget per-package install, got {result:?}"
    );
}
