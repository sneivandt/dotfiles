//! Shared test fixtures for APM task tests.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::engine::Context;
use crate::infra::ConfigHandle;
use crate::infra::env::MapEnv;
use crate::infra::exec::{CommandSpec, ExecResult, MockExecutor};
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::{empty_config, make_context};

use super::fragments::{discover_fragment_files, merge_fragments};
use super::install::InstallApmPackages;
use super::update::UpdateApmPackages;

pub const DEFAULT_FRAGMENT: &str =
    "name: base\nversion: 1.0.0\ndependencies:\n  apm:\n    - example/plugin\n";

pub fn write_home_fragment(home: &Path, filename: &str, content: &str) {
    let fragment_dir = home.join(".apm").join("config");
    std::fs::create_dir_all(&fragment_dir).expect("create fragment dir");
    std::fs::write(fragment_dir.join(filename), content).expect("write manifest fragment");
}

pub fn write_default_home_fragment(home: &Path) {
    write_home_fragment(home, "base.yml", DEFAULT_FRAGMENT);
}

pub fn write_current_manifest_and_lock(home: &Path) {
    write_default_home_fragment(home);
    let fragments = discover_fragment_files(home).expect("discover fragments");
    let merged = merge_fragments(&fragments).expect("merge fragments");
    std::fs::write(home.join(".apm").join("apm.yml"), merged).expect("write manifest");
    std::fs::write(
        home.join(".apm").join("apm.lock.yaml"),
        "dependencies: []\n",
    )
    .expect("write lock");
}

/// Compatibility name retained for autopilot fixtures; install markers no
/// longer exist because native APM owns convergence.
pub fn write_current_manifest_lock_and_marker(home: &Path) {
    write_current_manifest_and_lock(home);
}

pub fn write_copilot_app_db(home: &Path) -> PathBuf {
    let copilot_dir = home.join(".copilot");
    std::fs::create_dir_all(&copilot_dir).expect("create .copilot dir");
    let db_path = copilot_dir.join("data.db");
    std::fs::write(&db_path, b"db").expect("write data.db");
    db_path
}

pub fn make_context_with_home(home: &Path, platform: Platform, executor: MockExecutor) -> Context {
    make_context(
        empty_config(home.to_path_buf()),
        platform,
        Arc::new(executor),
    )
    .with_home(home.to_path_buf())
}

pub fn make_windows_cowork_context(home: &Path, executor: MockExecutor) -> Context {
    std::fs::create_dir_all(home.join(".agents").join("skills"))
        .expect("create shared APM skills target");
    let onedrive = home.join("OneDrive - Test");
    make_context_with_home(home, Platform::new(Os::Windows, false), executor).with_env(
        MapEnv::new()
            .with("USERPROFILE", home)
            .with("ONEDRIVECOMMERCIAL", &onedrive)
            .into_handle(),
    )
}

pub fn make_home_context_with_executor(home: &Path, executor: MockExecutor) -> Context {
    write_copilot_app_db(home);
    make_context_with_home(home, Platform::new(Os::Linux, false), executor)
}

pub fn install_task() -> InstallApmPackages {
    InstallApmPackages::new(ConfigHandle::new(Vec::new()))
}

pub fn update_task() -> UpdateApmPackages {
    UpdateApmPackages::new(ConfigHandle::new(Vec::new()))
}

pub fn expect_which_apm(mock: &mut MockExecutor, found: bool) {
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(move |_| found);
}

pub fn has_env(spec: &CommandSpec, key: &str, value: &str) -> bool {
    spec.environment()
        .iter()
        .any(|(actual_key, actual_value)| actual_key == key && actual_value == value)
}

pub fn expect_copilot_app_enable(mock: &mut MockExecutor, seq: &mut mockall::Sequence) {
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(|spec| {
            assert_eq!(spec.arguments(), ["experimental", "enable", "copilot-app"]);
            assert!(has_env(&spec, "GIT_TERMINAL_PROMPT", "0"));
            Ok(ExecResult::success("[!] copilot-app is already enabled.\n"))
        });
}

pub fn expect_cowork_enable(mock: &mut MockExecutor, seq: &mut mockall::Sequence) {
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(|spec| {
            assert_eq!(
                spec.arguments(),
                ["experimental", "enable", "copilot-cowork"]
            );
            Ok(ExecResult::success(
                "[!] copilot-cowork is already enabled.\n",
            ))
        });
}

pub fn expect_apm_update(
    mock: &mut MockExecutor,
    seq: &mut mockall::Sequence,
    stdout: &'static str,
) {
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(move |spec| {
            assert_eq!(spec.program(), "apm");
            assert_eq!(spec.arguments(), ["update", "-g", "--yes"]);
            Ok(ExecResult::success(stdout))
        });
}

pub fn expect_copilot_app_workflow_install(mock: &mut MockExecutor, seq: &mut mockall::Sequence) {
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(|spec| {
            assert_eq!(spec.program(), "apm");
            assert_eq!(
                spec.arguments(),
                ["install", "-g", "--target", "copilot-app", "--only", "apm"]
            );
            Ok(ExecResult::success("installed workflows\n"))
        });
}

pub fn expect_apm_install_without_enable(
    mock: &mut MockExecutor,
    seq: &mut mockall::Sequence,
    cwd: &Path,
) {
    let install_cwd = cwd.to_path_buf();
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(move |spec| {
            assert_eq!(spec.working_dir(), Some(install_cwd.as_path()));
            assert_eq!(spec.program(), "apm");
            assert_eq!(spec.arguments(), ["install", "-g"]);
            assert!(has_env(&spec, "GIT_TERMINAL_PROMPT", "0"));
            Ok(ExecResult::success("installed\n"))
        });
    expect_copilot_app_workflow_install(mock, seq);
}

pub fn expect_apm_install(mock: &mut MockExecutor, seq: &mut mockall::Sequence, cwd: &Path) {
    let install_cwd = cwd.to_path_buf();
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(move |spec| {
            assert_eq!(spec.working_dir(), Some(install_cwd.as_path()));
            assert_eq!(spec.arguments(), ["install", "-g"]);
            Ok(ExecResult::success("installed\n"))
        });
    expect_copilot_app_enable(mock, seq);
    expect_copilot_app_workflow_install(mock, seq);
}
