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
use super::manifest::write_manifest_marker;
use super::sources::install_fingerprint;
use super::targets::{ApmTargets, CopilotTarget};
use super::update::UpdateApmPackages;

/// Default APM fragment shared across APM test suites.
pub const DEFAULT_FRAGMENT: &str =
    "name: base\nversion: 1.0.0\ndependencies:\n  apm:\n    - example/plugin\n";

/// Write `content` into `<home>/.apm/config/<filename>`.
pub fn write_home_fragment(home: &Path, filename: &str, content: &str) {
    let fragment_dir = home.join(".apm").join("config");
    std::fs::create_dir_all(&fragment_dir).expect("create fragment dir");
    std::fs::write(fragment_dir.join(filename), content).expect("write manifest fragment");
}

/// Write the default fragment into `<home>/.apm/config/base.yml`.
pub fn write_default_home_fragment(home: &Path) {
    write_home_fragment(home, "base.yml", DEFAULT_FRAGMENT);
}

/// Write the default fragment, merged manifest, and bare lockfile under `home`.
pub fn write_current_manifest_and_lock(home: &Path) {
    write_default_home_fragment(home);
    let fragments = discover_fragment_files(home).expect("discover fragments");
    let merged = merge_fragments(&fragments).expect("merge fragments");
    std::fs::write(home.join(".apm").join("apm.yml"), merged).expect("write manifest");
    std::fs::write(home.join(".apm").join("apm.lock.yaml"), "lock\n").expect("write lock");
}

/// Write the default fragment, merged manifest, lockfile, and success marker
/// under `home`.
///
/// The marker uses the same fingerprint production writes, so a fixture-seeded
/// tree reads as "already installed" exactly when the real task would.
pub fn write_current_manifest_lock_and_marker(home: &Path) {
    write_current_manifest_and_lock(home);
    refresh_manifest_marker(home);
}

/// Stamp the success marker with the fingerprint the tree currently produces.
fn refresh_manifest_marker(home: &Path) {
    let manifest =
        std::fs::read_to_string(home.join(".apm").join("apm.yml")).expect("read manifest");
    let targets = if home.join(".copilot").join("data.db").exists() {
        ApmTargets::from_targets(&[CopilotTarget::CopilotApp])
    } else {
        ApmTargets::default()
    };
    write_manifest_marker(
        &home.join(".apm").join(".dotfiles-manifest.sha256"),
        &install_fingerprint(&manifest, home, targets).expect("fingerprint manifest"),
    )
    .expect("write marker");
}

/// Create `<home>/.copilot/data.db` so the copilot-app target and autopilot
/// fixup are enabled.  Returns the path to the created file.
pub fn write_copilot_app_db(home: &Path) -> PathBuf {
    let copilot_dir = home.join(".copilot");
    std::fs::create_dir_all(&copilot_dir).expect("create .copilot dir");
    let db_path = copilot_dir.join("data.db");
    std::fs::write(&db_path, b"db").expect("write data.db");
    // Enabling the copilot-app target changes the install fingerprint, so a
    // fixture that already seeded a success marker must re-stamp it to keep
    // reading as "already installed" regardless of helper call order.
    if home.join(".apm").join(".dotfiles-manifest.sha256").exists() {
        refresh_manifest_marker(home);
    }
    db_path
}

/// Build a [`Context`] rooted at `home` with the given platform and executor.
pub fn make_context_with_home(home: &Path, platform: Platform, executor: MockExecutor) -> Context {
    let ctx = make_context(
        empty_config(home.to_path_buf()),
        platform,
        Arc::new(executor),
    )
    .with_home(home.to_path_buf());
    if platform.is_windows() {
        std::fs::create_dir_all(home.join(".agents").join("skills"))
            .expect("create shared APM skills target");
        let onedrive = home.join("OneDrive - Test");
        ctx.with_env(
            MapEnv::new()
                .with("USERPROFILE", home)
                .with("ONEDRIVECOMMERCIAL", &onedrive)
                .into_handle(),
        )
    } else {
        ctx
    }
}

/// Build a Linux [`Context`] rooted at `home`, seeding `~/.copilot/data.db` so
/// the copilot-app target and autopilot fixup are enabled.
///
/// This is the shared "an install/update is about to run against a converged
/// or changed home" fixture used by both the top-level and autopilot suites.
pub fn make_home_context_with_executor(home: &Path, executor: MockExecutor) -> Context {
    write_copilot_app_db(home);
    make_context_with_home(home, Platform::new(Os::Linux, false), executor)
}

/// Construct an [`InstallApmPackages`] task with no configured fragment
/// sources; tests seed fragments directly under `home` instead.
pub fn install_task() -> InstallApmPackages {
    InstallApmPackages::new(ConfigHandle::new(Vec::new()))
}

/// Construct an [`UpdateApmPackages`] task with no configured fragment
/// sources; tests seed fragments directly under `home` instead.
pub fn update_task() -> UpdateApmPackages {
    UpdateApmPackages::new(ConfigHandle::new(Vec::new()))
}

/// Queue a `which("apm")` expectation, resolving to `found`.
pub fn expect_which_apm(mock: &mut MockExecutor, found: bool) {
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(move |_| found);
}

/// Does `spec`'s environment contain `key=value`?
pub fn has_env(spec: &CommandSpec, key: &str, value: &str) -> bool {
    spec.environment()
        .iter()
        .any(|(actual_key, actual_value)| actual_key == key && actual_value == value)
}

/// Queue the best-effort `apm experimental enable copilot-app` call.
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

/// Simulate APM writing a lockfile during a successful primary install.
pub fn write_empty_apm_lock(spec: &CommandSpec) {
    std::fs::write(
        spec.working_dir()
            .expect("APM install working directory")
            .join(".apm")
            .join("apm.lock.yaml"),
        "dependencies: []\n",
    )
    .expect("write APM lock fixture");
}

/// Queue `apm update -g --yes`, returning `stdout`.
///
/// Only checks `program`/`arguments`; use a bespoke closure instead when a
/// test also needs to assert `working_dir` or mutate the lockfile.
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

/// Queue `apm outdated -g` with the supplied stdout.
pub fn expect_apm_outdated(mock: &mut MockExecutor, cwd: &Path, stdout: &'static str) {
    let outdated_cwd = cwd.to_path_buf();
    mock.expect_execute().once().returning(move |spec| {
        assert_eq!(spec.working_dir(), Some(outdated_cwd.as_path()));
        assert_eq!(spec.program(), "apm");
        assert_eq!(spec.arguments(), ["outdated", "-g"]);
        assert!(has_env(&spec, "GIT_TERMINAL_PROMPT", "0"));
        assert!(has_env(&spec, "GCM_INTERACTIVE", "Never"));
        assert!(has_env(&spec, "GCM_GUI_PROMPT", "false"));
        Ok(ExecResult::success(stdout))
    });
}

/// Queue `apm outdated -g` in a larger ordered command sequence.
pub fn expect_apm_outdated_in_sequence(
    mock: &mut MockExecutor,
    seq: &mut mockall::Sequence,
    cwd: &Path,
    stdout: &'static str,
) {
    let outdated_cwd = cwd.to_path_buf();
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(move |spec| {
            assert_eq!(spec.working_dir(), Some(outdated_cwd.as_path()));
            assert_eq!(spec.program(), "apm");
            assert_eq!(spec.arguments(), ["outdated", "-g"]);
            Ok(ExecResult::success(stdout))
        });
}

/// Queue the separate Copilot App workflow deploy (`apm install -g --target
/// copilot-app`) that follows every apm install/update cycle.
pub fn expect_copilot_app_workflow_install(mock: &mut MockExecutor, seq: &mut mockall::Sequence) {
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(|spec| {
            assert_eq!(spec.program(), "apm");
            assert_eq!(
                spec.arguments(),
                ["install", "-g", "--target", "copilot-app"]
            );
            Ok(ExecResult::success("installed workflows\n"))
        });
}

/// Queue `apm prune`, run from `<cwd>/.apm`.
pub fn expect_apm_prune(mock: &mut MockExecutor, seq: &mut mockall::Sequence, cwd: &Path) {
    let prune_cwd = cwd.join(".apm");
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(move |spec| {
            assert_eq!(spec.working_dir(), Some(prune_cwd.as_path()));
            assert_eq!(spec.program(), "apm");
            assert_eq!(spec.arguments(), ["prune"]);
            assert!(has_env(&spec, "GIT_TERMINAL_PROMPT", "0"));
            Ok(ExecResult::success("pruned\n"))
        });
}

/// Queue the install/deploy/prune tail of an apm install run: `install -g`,
/// the separate copilot-app target deploy, and `apm prune` -- everything
/// after `apm experimental enable`.
pub fn expect_apm_install_without_enable(
    mock: &mut MockExecutor,
    seq: &mut mockall::Sequence,
    cwd: &Path,
) {
    let install_cwd = cwd.to_path_buf();
    let copilot_app_cwd = install_cwd.clone();
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(move |spec| {
            assert_eq!(spec.working_dir(), Some(copilot_app_cwd.as_path()));
            assert_eq!(spec.program(), "apm");
            assert_eq!(spec.arguments(), ["install", "-g"]);
            assert!(has_env(&spec, "GIT_TERMINAL_PROMPT", "0"));
            assert!(has_env(&spec, "GCM_INTERACTIVE", "Never"));
            assert!(has_env(&spec, "GCM_GUI_PROMPT", "false"));
            Ok(ExecResult::success("installed\n"))
        });
    mock.expect_execute()
        .once()
        .in_sequence(seq)
        .returning(move |spec| {
            assert_eq!(spec.working_dir(), Some(install_cwd.as_path()));
            assert_eq!(spec.program(), "apm");
            assert_eq!(
                spec.arguments(),
                ["install", "-g", "--target", "copilot-app"]
            );
            assert!(has_env(&spec, "GIT_TERMINAL_PROMPT", "0"));
            Ok(ExecResult::success("installed workflows\n"))
        });
    expect_apm_prune(mock, seq, cwd);
}

/// Queue the full apm install sequence: experimental-enable, install, the
/// copilot-app target deploy, and prune.
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
    expect_apm_prune(mock, seq, cwd);
}
