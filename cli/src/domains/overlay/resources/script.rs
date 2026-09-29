//! Overlay script resource.
//!
//! Runs custom scripts from a private overlay repository.  Scripts follow a
//! convention-based interface:
//!
//! - **Check**: Run the script with `--check`.  Exit code 0 means the resource
//!   is in the correct state; exit code 1 means it needs to be applied; any
//!   other non-zero status is treated as a check failure.
//! - **Apply**: Run the script with no arguments to apply the desired state.
//! - **Dry-run**: Run the script with `--dryrun` to preview changes without
//!   mutating state.
//! - **Remove**: Run the script with `--remove` to undo the applied state.
//!
//! Dry-run safety is cooperative for these opaque external scripts: the engine
//! passes `--check` and `--dryrun`, but cannot prevent a script from mutating
//! state if it violates that contract.
//!
//! `PowerShell` scripts (`.ps1`) are invoked via `pwsh`/`powershell`, shell
//! scripts (`.sh`) are invoked via `sh`.
use anyhow::{Context as _, Result, bail};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::engine::resource::ResourceError;
use crate::engine::{
    IntrinsicState, RemovableResource, Resource, ResourceChange, ResourceResult, ResourceState,
};
use crate::infra::exec::{CommandSpec, Executor};

/// A resource that runs a custom script from an overlay repository.
#[derive(Debug)]
pub struct ScriptResource {
    /// Human-readable name for this script.
    name: String,
    /// Absolute path to the script file.
    script_path: PathBuf,
    /// Working directory for script execution (overlay root).
    working_dir: PathBuf,
    /// Command executor.
    executor: Arc<dyn Executor>,
}

impl ScriptResource {
    /// Create a new script resource.
    #[must_use]
    pub fn new(
        name: String,
        script_path: PathBuf,
        working_dir: PathBuf,
        executor: Arc<dyn Executor>,
    ) -> Self {
        Self {
            name,
            script_path,
            working_dir,
            executor,
        }
    }

    /// Build a script resource from a config entry and overlay root.
    ///
    /// # Errors
    ///
    /// Returns an error when the configured script path is absolute or escapes
    /// the overlay root.
    pub fn from_entry(
        entry: &crate::domains::overlay::config::scripts::ScriptEntry,
        overlay_root: &Path,
        executor: Arc<dyn Executor>,
    ) -> Result<Self> {
        let script_path =
            crate::domains::overlay::config::scripts::resolve_script_path(entry, overlay_root)?;
        Ok(Self::new(
            entry.name.clone(),
            script_path,
            overlay_root.to_path_buf(),
            executor,
        ))
    }

    /// Run the script in apply mode and return its captured stdout.
    ///
    /// # Errors
    ///
    /// Returns an error when the script path is invalid, its interpreter is
    /// unavailable, or execution fails.
    pub fn apply_with_output(&self) -> Result<(ResourceChange, String)> {
        self.execute(ScriptMode::Apply)
    }

    /// Run the script in dry-run mode and return its captured stdout.
    ///
    /// # Errors
    ///
    /// Returns an error when the script path is invalid, its interpreter is
    /// unavailable, or execution fails.
    pub fn preview_with_output(&self) -> Result<(ResourceChange, String)> {
        self.execute(ScriptMode::DryRun)
    }

    /// Remove the script's managed state and return its captured stdout.
    ///
    /// # Errors
    ///
    /// Returns an error when the script cannot be run or removal fails.
    pub fn remove_with_output(&self) -> Result<(ResourceChange, String)> {
        self.execute(ScriptMode::Remove)
    }

    /// Build the complete command request for a mode.
    fn command(&self, flag: Option<&str>) -> Result<CommandSpec> {
        let (interpreter, fixed_args) = interpreter_args_for(&self.script_path, &*self.executor)?;
        let mut command = CommandSpec::new(interpreter)
            .args(&fixed_args)
            .arg(self.script_path.display().to_string())
            .current_dir(&self.working_dir);
        if let Some(flag) = flag {
            command = command.arg(flag);
        }
        Ok(command)
    }

    fn execute(&self, mode: ScriptMode) -> Result<(ResourceChange, String)> {
        if !self.script_path.exists() {
            return Ok((
                ResourceChange::skipped(format!(
                    "script not found: {}",
                    self.script_path.display()
                )),
                String::new(),
            ));
        }
        self.ensure_script_path_within_working_dir()?;

        let result = self
            .executor
            .execute(self.command(mode.flag())?)
            .with_context(|| format!("{} script: {}", mode.action(), self.name))?;

        Ok((ResourceChange::Applied, result.stdout))
    }
}

#[derive(Debug, Clone, Copy)]
enum ScriptMode {
    Apply,
    DryRun,
    Remove,
}

impl ScriptMode {
    const fn flag(self) -> Option<&'static str> {
        match self {
            Self::Apply => None,
            Self::DryRun => Some("--dryrun"),
            Self::Remove => Some("--remove"),
        }
    }

    const fn action(self) -> &'static str {
        match self {
            Self::Apply => "running",
            Self::DryRun => "dry-run",
            Self::Remove => "removing",
        }
    }
}

impl Resource for ScriptResource {
    fn description(&self) -> String {
        self.name.clone()
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        self.execute(ScriptMode::Apply)
            .map(|(change, _output)| change)
            .map_err(ResourceError::from)
    }
}

impl RemovableResource for ScriptResource {
    fn remove(&self) -> ResourceResult<ResourceChange> {
        self.remove_with_output()
            .map(|(change, _output)| change)
            .map_err(ResourceError::from)
    }
}

impl IntrinsicState for ScriptResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        if !self.script_path.exists() {
            return Ok(ResourceState::Invalid {
                reason: format!("script not found: {}", self.script_path.display()),
            });
        }
        self.ensure_script_path_within_working_dir()?;

        let result = self
            .executor
            .execute(self.command(Some("--check"))?.unchecked())
            .with_context(|| format!("checking script state: {}", self.name))?;

        match (result.success, result.code) {
            (true, _) => Ok(ResourceState::Correct),
            (false, Some(1)) => Ok(ResourceState::Missing),
            (false, code) => Ok(ResourceState::Unknown {
                reason: format_check_failure(&self.name, code, &result.stdout, &result.stderr),
            }),
        }
    }
}

impl ScriptResource {
    fn ensure_script_path_within_working_dir(&self) -> Result<()> {
        ensure_script_path_within(&self.working_dir, &self.script_path)
    }
}

/// Determine the interpreter and fixed arguments for an overlay script.
///
/// # Errors
///
/// Returns an error when a PowerShell script cannot be run on the current
/// platform because the required shell is unavailable.
pub(crate) fn interpreter_args_for(
    script_path: &Path,
    executor: &dyn Executor,
) -> Result<(&'static str, Vec<&'static str>)> {
    let ext = script_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    if ext.eq_ignore_ascii_case("ps1") {
        let shell = powershell_interpreter(executor)?;
        Ok((
            shell,
            vec![
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ],
        ))
    } else {
        Ok(("sh", vec![]))
    }
}

fn powershell_interpreter(executor: &dyn Executor) -> Result<&'static str> {
    if executor.which("pwsh") {
        return Ok("pwsh");
    }
    if cfg!(windows) && executor.which("powershell") {
        return Ok("powershell");
    }

    let reason = if cfg!(windows) {
        "PowerShell scripts require 'pwsh' or 'powershell' on Windows"
    } else {
        "PowerShell scripts require 'pwsh' on non-Windows platforms"
    };
    Err(ResourceError::not_supported(reason).into())
}

/// Ensure the resolved script path is inside the overlay root.
///
/// # Errors
///
/// Returns an error when either path cannot be canonicalized or the script
/// resolves outside the overlay root.
pub(crate) fn ensure_script_path_within(working_dir: &Path, script_path: &Path) -> Result<()> {
    let root = working_dir
        .canonicalize()
        .with_context(|| format!("resolve overlay root: {}", working_dir.display()))?;
    let script = script_path
        .canonicalize()
        .with_context(|| format!("resolve script path: {}", script_path.display()))?;

    if !script.starts_with(&root) {
        bail!(
            "script path escapes overlay root: {} is outside {}",
            script.display(),
            root.display()
        );
    }

    Ok(())
}

fn format_check_failure(name: &str, code: Option<i32>, stdout: &str, stderr: &str) -> String {
    let status = code.map_or_else(
        || "terminated by signal".to_string(),
        |c| format!("exit {c}"),
    );
    let stdout = stdout.trim();
    let stderr = stderr.trim();
    let detail = match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => "no output".to_string(),
        (false, true) => format!("stdout: {stdout}"),
        (true, false) => stderr.to_string(),
        (false, false) => format!("stdout: {stdout}; stderr: {stderr}"),
    };
    format!("script check failed for {name} ({status}): {detail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::exec::{ExecError, ExecResult, MockExecutor};

    fn make_script_resource(
        name: &str,
        path: &Path,
        executor: Arc<dyn Executor>,
    ) -> ScriptResource {
        ScriptResource::new(
            name.to_string(),
            path.to_path_buf(),
            path.parent()
                .unwrap_or_else(|| Path::new("/"))
                .to_path_buf(),
            executor,
        )
    }

    #[test]
    fn missing_script_is_invalid_and_never_invoked_by_any_mode() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let resource = make_script_resource(
            "test",
            &dir.path().join("missing.ps1"),
            Arc::new(MockExecutor::new()),
        );
        let state = resource.current_state().unwrap();
        assert!(matches!(state, ResourceState::Invalid { .. }));
        for mode in [ScriptMode::Apply, ScriptMode::DryRun, ScriptMode::Remove] {
            let (change, output) = resource.execute(mode).unwrap();
            assert!(matches!(change, ResourceChange::Skipped { .. }), "{mode:?}");
            assert!(output.is_empty(), "{mode:?}");
        }
    }

    #[test]
    fn interpreter_uses_sh_for_shell_scripts() {
        let mock = Arc::new(MockExecutor::new());
        let resource = make_script_resource("test", Path::new("/scripts/test.sh"), mock);
        for flag in [None, Some("--check"), Some("--dryrun"), Some("--remove")] {
            let command = resource.command(flag).unwrap();
            let mut args = vec!["/scripts/test.sh"];
            args.extend(flag);
            assert_eq!(command.program(), "sh", "{flag:?}");
            assert_eq!(command.arguments(), args, "{flag:?}");
            assert_eq!(
                command.working_dir(),
                Some(Path::new("/scripts")),
                "{flag:?}"
            );
            assert!(command.is_checked(), "{flag:?}");
        }
    }

    #[test]
    #[cfg(windows)]
    fn interpreter_uses_powershell_for_ps1_scripts() {
        // Mock executor does not find pwsh on PATH, so falls back to powershell
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .once()
            .withf(|p: &str| p == "pwsh")
            .returning(|_| false);
        mock.expect_which()
            .once()
            .withf(|p: &str| p == "powershell")
            .returning(|_| true);
        let mock = Arc::new(mock);
        let resource = make_script_resource("test", Path::new("/scripts/test.ps1"), mock);
        let command = resource.command(None).unwrap();
        assert_eq!(command.program(), "powershell");
        assert_eq!(
            command.arguments(),
            [
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                "/scripts/test.ps1"
            ]
        );
        assert!(command.is_checked());
        assert_eq!(command.working_dir(), Some(Path::new("/scripts")));
    }

    #[test]
    #[cfg(not(windows))]
    fn interpreter_requires_pwsh_for_ps1_scripts_off_windows() {
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .once()
            .withf(|p: &str| p == "pwsh")
            .returning(|_| false);
        let mock = Arc::new(mock);
        let resource = make_script_resource("test", Path::new("/scripts/test.ps1"), mock);
        let err = resource
            .command(None)
            .expect_err("missing pwsh should fail off Windows");
        assert!(
            err.to_string()
                .contains("PowerShell scripts require 'pwsh'")
        );
    }

    #[test]
    fn interpreter_prefers_pwsh_for_ps1_scripts_when_available() {
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .times(4)
            .withf(|p: &str| p == "pwsh")
            .returning(|_| true);
        let mock = Arc::new(mock);
        let resource = make_script_resource("test", Path::new("/scripts/test.ps1"), mock);
        for flag in [None, Some("--check"), Some("--dryrun"), Some("--remove")] {
            let command = resource.command(flag).unwrap();
            let mut expected = vec![
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                "/scripts/test.ps1",
            ];
            expected.extend(flag);
            assert_eq!(command.program(), "pwsh");
            assert_eq!(command.arguments(), expected, "{flag:?}");
            assert_eq!(command.working_dir(), Some(Path::new("/scripts")));
            assert!(command.is_checked());
        }
    }

    #[test]
    fn interpreter_recognizes_powershell_extensions_without_case_sensitivity() {
        for filename in ["setup.PS1", "setup.Ps1", "setup.pS1"] {
            let mut mock = MockExecutor::new();
            mock.expect_which()
                .times(4)
                .withf(|program| program == "pwsh")
                .return_const(true);
            let resource = make_script_resource("test", Path::new(filename), Arc::new(mock));
            for flag in [None, Some("--check"), Some("--dryrun"), Some("--remove")] {
                let command = resource.command(flag).unwrap();
                let mut expected = vec![
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                    filename,
                ];
                expected.extend(flag);
                assert_eq!(command.program(), "pwsh", "{filename} {flag:?}");
                assert_eq!(command.arguments(), expected, "{filename} {flag:?}");
                assert!(command.is_checked());
            }
        }
    }

    #[test]
    fn from_entry_resolves_path() {
        let mock = Arc::new(MockExecutor::new());
        let entry = crate::domains::overlay::config::scripts::ScriptEntry {
            name: "Setup database".to_string(),
            path: "scripts/setup-db.ps1".to_string(),
            description: None,
        };
        let resource = ScriptResource::from_entry(&entry, Path::new("/overlay"), mock)
            .expect("relative script path should create resource");
        assert_eq!(
            resource.script_path,
            PathBuf::from("/overlay/scripts/setup-db.ps1")
        );
        assert_eq!(resource.working_dir, PathBuf::from("/overlay"));
    }

    #[test]
    fn from_entry_rejects_script_path_traversal() {
        let mock = Arc::new(MockExecutor::new());
        let entry = crate::domains::overlay::config::scripts::ScriptEntry {
            name: "Setup database".to_string(),
            path: "../setup-db.ps1".to_string(),
            description: None,
        };
        let err = ScriptResource::from_entry(&entry, Path::new("/overlay"), mock)
            .expect_err("path traversal should be rejected");
        assert!(err.to_string().contains("must not contain '..'"));
    }

    #[test]
    fn current_state_maps_only_documented_check_statuses() {
        for (result, expected) in [
            (ExecResult::success(""), ResourceState::Correct),
            (ExecResult::failure("", "", Some(1)), ResourceState::Missing),
            (
                ExecResult::failure("", " syntax error \n", Some(2)),
                ResourceState::Unknown {
                    reason: "script check failed for test (exit 2): syntax error".to_string(),
                },
            ),
            (
                ExecResult::failure(" partial output \n", " interrupted \n", None),
                ResourceState::Unknown {
                    reason: "script check failed for test (terminated by signal): stdout: partial output; stderr: interrupted".to_string(),
                },
            ),
            (
                ExecResult::failure("", "", Some(3)),
                ResourceState::Unknown {
                    reason: "script check failed for test (exit 3): no output".to_string(),
                },
            ),
        ] {
            let dir = tempfile::tempdir_in(".").unwrap();
            let script_path = dir.path().join("test.sh");
            std::fs::write(&script_path, "#!/bin/sh\n").unwrap();
            let expected_path = script_path.clone();
            let expected_root = dir.path().to_path_buf();
            let mut mock = MockExecutor::new();
            mock.expect_execute()
                .once()
                .withf(move |spec| {
                    spec.program() == "sh"
                        && spec.arguments() == [expected_path.as_os_str(), std::ffi::OsStr::new("--check")]
                        && spec.working_dir() == Some(expected_root.as_path())
                        && !spec.is_checked()
                })
                .return_once(|_| Ok(result));
            let resource = make_script_resource("test", &script_path, Arc::new(mock));

            assert_eq!(resource.current_state().unwrap(), expected);
        }
    }

    #[test]
    fn execution_modes_propagate_checked_failures_with_context() {
        for (mode, flag, context) in [
            (ScriptMode::Apply, None, "running script: test"),
            (ScriptMode::DryRun, Some("--dryrun"), "dry-run script: test"),
            (
                ScriptMode::Remove,
                Some("--remove"),
                "removing script: test",
            ),
        ] {
            let dir = tempfile::tempdir_in(".").unwrap();
            let script_path = dir.path().join("test.sh");
            std::fs::write(&script_path, "#!/bin/sh\n").unwrap();
            let mut expected_args = vec![script_path.as_os_str().to_os_string()];
            expected_args.extend(flag.map(std::ffi::OsString::from));
            let expected_root = dir.path().to_path_buf();
            let mut mock = MockExecutor::new();
            mock.expect_execute()
                .once()
                .withf(move |spec| {
                    spec.program() == "sh"
                        && spec.arguments() == expected_args
                        && spec.working_dir() == Some(expected_root.as_path())
                        && spec.is_checked()
                })
                .returning(|_| {
                    Err(ExecError::non_zero(
                        "sh test.sh",
                        ExecResult::failure("", "fixture failure", Some(2)),
                    ))
                });
            let resource = make_script_resource("test", &script_path, Arc::new(mock));

            let error = resource.execute(mode).unwrap_err();
            assert!(error.to_string().contains(context), "{mode:?}: {error:#}");
            assert!(
                format!("{error:#}").contains("fixture failure"),
                "{error:#}"
            );
        }
    }
}
