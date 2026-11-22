//! Shared test fixtures for APM task tests.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::engine::Context;
use crate::infra::ConfigHandle;
use crate::infra::env::MapEnv;
use crate::infra::exec::{CommandSpec, ExecError, ExecResult, MockExecutor};
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::make_context;

use super::fragments::{discover_fragment_files, merge_fragments};
use super::install::{ApmPackageMode, InstallApmPackages};

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

pub fn write_copilot_app_db(home: &Path) -> PathBuf {
    let copilot_dir = home.join(".copilot");
    std::fs::create_dir_all(&copilot_dir).expect("create .copilot dir");
    let db_path = copilot_dir.join("data.db");
    std::fs::write(&db_path, b"db").expect("write data.db");
    db_path
}

pub fn make_context_with_home(home: &Path, platform: Platform, executor: MockExecutor) -> Context {
    make_context(home.to_path_buf(), None, platform, Arc::new(executor))
        .with_home(home.to_path_buf())
        .with_env(MapEnv::new().into_handle())
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
    InstallApmPackages::new(ConfigHandle::new(Vec::new()), ApmPackageMode::Install)
}

pub fn update_task() -> InstallApmPackages {
    InstallApmPackages::new(ConfigHandle::new(Vec::new()), ApmPackageMode::UpdatePins)
}

pub fn expect_which_apm(mock: &mut MockExecutor, found: bool) {
    mock.expect_which()
        .with(mockall::predicate::eq("apm"))
        .once()
        .returning(move |_| found);
}

fn has_env(spec: &CommandSpec, key: &str, value: &str) -> bool {
    spec.environment()
        .iter()
        .any(|(actual_key, actual_value)| actual_key == key && actual_value == value)
}

pub fn assert_apm_command(spec: &CommandSpec) {
    assert_eq!(spec.program(), "apm");
    assert!(
        spec.is_checked(),
        "APM failures must not be mistaken for success"
    );
    assert!(
        spec.working_dir().is_some(),
        "APM must run in the fixture home"
    );
    assert!(has_env(spec, "GIT_TERMINAL_PROMPT", "0"));
    for (key, value) in super::commands::APM_NONINTERACTIVE_ENV {
        assert!(
            has_env(spec, key, value),
            "missing noninteractive setting {key}"
        );
    }
}

pub const INSTALL: &[&str] = &["install", "-g"];
pub const UPDATE: &[&str] = &["update", "-g", "--yes"];
pub const PREVIEW_UPDATE: &[&str] = &["update", "-g", "--dry-run"];

/// Ordered native commands (and, for Copilot App, workflow snapshots and repairs)
/// sharing the same isolated home. No other programs or calls are accepted.
pub struct ApmRun<'a> {
    pub home: &'a Path,
    pub mock: MockExecutor,
    pub seq: mockall::Sequence,
}

impl<'a> ApmRun<'a> {
    pub fn new(home: &'a Path, apm_probes: usize) -> Self {
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .with(mockall::predicate::eq("apm"))
            .times(apm_probes)
            .return_const(true);
        Self {
            home,
            mock,
            seq: mockall::Sequence::new(),
        }
    }

    pub fn command(
        &mut self,
        args: &[&str],
        respond: impl FnOnce(&Path) -> Result<ExecResult, ExecError> + Send + 'static,
    ) {
        let home = self.home.to_owned();
        let args: Vec<_> = args.iter().map(std::ffi::OsString::from).collect();
        self.mock
            .expect_execute()
            .once()
            .in_sequence(&mut self.seq)
            .return_once(move |spec| {
                assert_apm_command(&spec);
                assert_eq!(spec.working_dir(), Some(home.as_path()));
                assert_eq!(spec.arguments(), args);
                respond(&home)
            });
    }

    pub fn ok(&mut self, args: &[&str], stdout: &'static str) {
        self.command(args, move |_| Ok(ExecResult::success(stdout)));
    }

    pub fn enable(&mut self, target: &'static str) {
        self.command(&["experimental", "enable", target], move |_| {
            Ok(ExecResult::success(format!(
                "[!] {target} is already enabled.\n"
            )))
        });
    }

    pub fn workflow_install(&mut self) {
        self.ok(
            &["install", "-g", "--target", "copilot-app", "--only", "apm"],
            "installed workflows\n",
        );
    }

    pub fn copilot_install(&mut self) {
        self.ok(INSTALL, "installed\n");
        self.enable("copilot-app");
        self.workflow_install();
    }

    pub fn copilot_update(&mut self, stdout: &'static str) {
        self.ok(UPDATE, stdout);
        self.ok(INSTALL, "converged deployments\n");
        self.enable("copilot-app");
        self.workflow_install();
    }
}
