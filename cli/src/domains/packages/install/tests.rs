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

mod native;
mod paru;
mod readiness;
mod winget;
