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

fn paru_path() -> PathBuf {
    PathBuf::from("/usr/bin/paru")
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

fn expect_installed_paru_package(mock: &mut MockExecutor, times: usize) {
    mock.expect_execute()
        .times(times)
        .withf(is_paru_package_query)
        .returning(|_| Ok(ExecResult::success("paru 2.1.0-2\n")));
}

fn expect_healthy_paru(mock: &mut MockExecutor, times: usize) {
    mock.expect_execute()
        .times(times)
        .withf(is_paru_version)
        .returning(|_| Ok(ExecResult::success("paru v2.1.0 - libalpm v16.0.1\n")));
}

fn make_package_context(
    root: PathBuf,
    overlay: Option<PathBuf>,
    os: Os,
    is_arch: bool,
    executor: impl Executor + 'static,
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

#[test]
fn pacman_inventory_cache_invalidate_and_paru_wiring_and_preview_warning() {
    let cache = PacmanInventoryCache::new();
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .times(2)
        .withf(is_native_inventory)
        .returning(|_| Ok(ExecResult::success("git\n")));
    assert!(
        cache
            .query(InventoryPhase::Execution, PackageManager::Pacman, &mock)
            .unwrap()
            .contains_package("git")
    );
    assert!(
        cache
            .query(InventoryPhase::Execution, PackageManager::Pacman, &mock)
            .unwrap()
            .contains_package("git")
    );
    cache.invalidate();
    assert!(
        cache
            .query(InventoryPhase::Execution, PackageManager::Pacman, &mock)
            .unwrap()
            .contains_package("git")
    );

    let paru = InstallParu::new().with_inventory_cache(cache);
    assert!(paru.inventory_cache.is_some());

    let log = Arc::new(CapturingOutput::default());
    let log_clone = Arc::clone(&log);
    let log_handle: Arc<dyn crate::infra::logging::Log> = log_clone;
    let ctx = Context::from_raw(
        PathBuf::from("/tmp"),
        None,
        crate::infra::platform::Platform::new(Os::Linux, true),
        log_handle,
        Arc::new(MockExecutor::new()),
        PathBuf::from("/home/test"),
        crate::engine::ContextOpts {
            dry_run: true,
            ..Default::default()
        },
    );
    let op = PackageInstallOperation {
        manager: PackageManager::Pacman,
        packages: vec![],
        inventory_cache: None,
    };
    let plan = PackageInstallPlan {
        already_ok: 1,
        missing: vec![],
        unavailable: vec![("missing-pkg".into(), "not built".into())],
    };
    let result = op.preview(&ctx, &plan).unwrap();
    assert!(!matches!(result, TaskResult::Skipped { .. }));
    let warnings = log.messages_of(crate::infra::logging::MsgKind::Warn);
    assert!(
        !warnings.iter().any(|msg| msg.contains("pacman -Syu")),
        "preview must not warn about pacman -Syu when no packages are missing: {warnings:?}"
    );
}

#[test]
fn predict_sudo_empty_packages_and_custom_package_name_matching() {
    // predict_sudo returns false without querying when tool exists but packages is empty.
    let mut mock_empty = MockExecutor::new();
    mock_empty.expect_which().once().returning(|_| true);
    let ctx_empty = crate::test_helpers::make_context(
        PathBuf::from("/tmp"),
        None,
        crate::infra::platform::Platform::new(Os::Linux, true),
        Arc::new(mock_empty),
    );
    assert!(!predict_sudo(
        &ctx_empty,
        PackageManager::Pacman,
        "pacman",
        &[],
        None,
    ));

    // build_install_plan matches a single requested custom package by exact name (kills == -> !=).
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let pkg_dir = dir.path().join("packages/apm-dotfiles");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        pkg_dir.join("release.json"),
        r#"{"package_base":"apm-dotfiles","repository":"microsoft/apm","assets":{"x86_64":"apm.tar.gz","aarch64":"apm.tar.gz"}}"#,
    )
    .unwrap();
    std::fs::write(pkg_dir.join("PKGBUILD"), "pkgname=apm-dotfiles\npkgrel=1\n").unwrap();
    let mut mock_paru = MockExecutor::new();
    mock_paru
        .expect_execute()
        .once()
        .returning(|_| Ok(ExecResult::success("")));
    let ctx_paru = Context::from_raw(
        dir.path().to_path_buf(),
        None,
        crate::infra::platform::Platform::new(Os::Linux, true),
        Arc::new(crate::infra::logging::Logger::test()),
        Arc::new(mock_paru),
        home,
        crate::engine::ContextOpts::default(),
    );
    let custom_plan = build_install_plan(
        &ctx_paru,
        &[Package {
            name: "apm-dotfiles".into(),
            is_aur: true,
        }],
        PackageManager::Paru,
        None,
    )
    .unwrap();
    assert_eq!(custom_plan.unavailable.len(), 1, "{custom_plan:?}");
    assert_eq!(custom_plan.unavailable[0].0, "apm-dotfiles");
    assert!(custom_plan.missing.is_empty(), "{custom_plan:?}");
}

mod native;
mod paru;
mod readiness;
mod winget;
