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

use super::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::error::ResourceError;
use crate::exec::Executor;

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
        entry: &crate::config::scripts::ScriptEntry,
        overlay_root: &Path,
        executor: Arc<dyn Executor>,
    ) -> Result<Self> {
        let script_path = crate::config::scripts::resolve_script_path(entry, overlay_root)?;
        Ok(Self::new(
            entry.name.clone(),
            script_path,
            overlay_root.to_path_buf(),
            executor,
        ))
    }

    /// Determine the interpreter and arguments for the script based on its extension.
    fn interpreter_args(&self) -> Result<(&str, Vec<&str>)> {
        interpreter_args_for(&self.script_path, &*self.executor)
    }
}

impl Resource for ScriptResource {
    fn description(&self) -> String {
        self.name.clone()
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        if !self.script_path.exists() {
            return Ok(ResourceChange::Skipped {
                reason: format!("script not found: {}", self.script_path.display()),
            });
        }
        self.ensure_script_path_within_working_dir()?;

        let (interpreter, mut args) = self.interpreter_args()?;
        let script_str = self.script_path.display().to_string();
        args.push(&script_str);

        self.executor
            .run_in(&self.working_dir, interpreter, &args)
            .with_context(|| format!("running script: {}", self.name))?;

        Ok(ResourceChange::Applied)
    }

    fn remove(&self) -> ResourceResult<ResourceChange> {
        if !self.script_path.exists() {
            return Ok(ResourceChange::Skipped {
                reason: format!("script not found: {}", self.script_path.display()),
            });
        }
        self.ensure_script_path_within_working_dir()?;

        let (interpreter, mut args) = self.interpreter_args()?;
        let script_str = self.script_path.display().to_string();
        args.push(&script_str);
        args.push("--remove");

        self.executor
            .run_in(&self.working_dir, interpreter, &args)
            .with_context(|| format!("removing script: {}", self.name))?;

        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for ScriptResource {
    fn current_state(&self) -> Result<ResourceState> {
        if !self.script_path.exists() {
            return Ok(ResourceState::Invalid {
                reason: format!("script not found: {}", self.script_path.display()),
            });
        }
        self.ensure_script_path_within_working_dir()?;

        let (interpreter, mut args) = self.interpreter_args()?;
        let script_str = self.script_path.display().to_string();
        args.push(&script_str);
        args.push("--check");

        let result = self
            .executor
            .run_unchecked_in(&self.working_dir, interpreter, &args)
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

    match ext {
        "ps1" => {
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
        }
        _ => Ok(("sh", vec![])),
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
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::exec::{ExecResult, MockExecutor};

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
    fn description_returns_name() {
        let mock = Arc::new(MockExecutor::new());
        let resource =
            make_script_resource("Setup database", Path::new("/scripts/setup-db.ps1"), mock);
        assert_eq!(resource.description(), "Setup database");
    }

    #[test]
    fn current_state_returns_invalid_when_script_missing() {
        let mock = Arc::new(MockExecutor::new());
        let resource = make_script_resource("test", Path::new("/nonexistent.ps1"), mock);
        let state = resource.current_state().unwrap();
        assert!(matches!(state, ResourceState::Invalid { .. }));
    }

    #[test]
    fn apply_returns_skipped_when_script_missing() {
        let mock = Arc::new(MockExecutor::new());
        let resource = make_script_resource("test", Path::new("/nonexistent.ps1"), mock);
        let result = resource.apply().unwrap();
        assert!(matches!(result, ResourceChange::Skipped { .. }));
    }

    #[test]
    fn interpreter_uses_sh_for_shell_scripts() {
        let mock = Arc::new(MockExecutor::new());
        let resource = make_script_resource("test", Path::new("/scripts/test.sh"), mock);
        let (interpreter, _) = resource.interpreter_args().unwrap();
        assert_eq!(interpreter, "sh");
    }

    #[test]
    #[cfg(windows)]
    fn interpreter_uses_powershell_for_ps1_scripts() {
        // Mock executor does not find pwsh on PATH, so falls back to powershell
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .withf(|p: &str| p == "pwsh")
            .returning(|_| false);
        mock.expect_which()
            .withf(|p: &str| p == "powershell")
            .returning(|_| true);
        let mock = Arc::new(mock);
        let resource = make_script_resource("test", Path::new("/scripts/test.ps1"), mock);
        let (interpreter, args) = resource.interpreter_args().unwrap();
        assert_eq!(interpreter, "powershell");
        assert!(args.contains(&"-File"));
    }

    #[test]
    #[cfg(not(windows))]
    fn interpreter_requires_pwsh_for_ps1_scripts_off_windows() {
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .withf(|p: &str| p == "pwsh")
            .returning(|_| false);
        let mock = Arc::new(mock);
        let resource = make_script_resource("test", Path::new("/scripts/test.ps1"), mock);
        let err = resource
            .interpreter_args()
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
            .withf(|p: &str| p == "pwsh")
            .returning(|_| true);
        let mock = Arc::new(mock);
        let resource = make_script_resource("test", Path::new("/scripts/test.ps1"), mock);
        let (interpreter, args) = resource.interpreter_args().unwrap();
        assert_eq!(interpreter, "pwsh");
        assert!(args.contains(&"-File"));
    }

    #[test]
    fn from_entry_resolves_path() {
        let mock = Arc::new(MockExecutor::new());
        let entry = crate::config::scripts::ScriptEntry {
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
        let entry = crate::config::scripts::ScriptEntry {
            name: "Setup database".to_string(),
            path: "../setup-db.ps1".to_string(),
            description: None,
        };
        let err = ScriptResource::from_entry(&entry, Path::new("/overlay"), mock)
            .expect_err("path traversal should be rejected");
        assert!(err.to_string().contains("must not contain '..'"));
    }

    #[test]
    fn current_state_treats_check_exit_one_as_missing() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("test.sh");
        std::fs::write(&script_path, "#!/bin/sh\n").unwrap();
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked_in().once().returning(|_, _, _| {
            Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                success: false,
                code: Some(1),
            })
        });
        let resource = ScriptResource::new(
            "test".to_string(),
            script_path,
            dir.path().to_path_buf(),
            Arc::new(mock),
        );
        let state = resource.current_state().unwrap();
        assert!(matches!(state, ResourceState::Missing));
    }

    #[test]
    fn current_state_treats_other_check_failures_as_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("test.sh");
        std::fs::write(&script_path, "#!/bin/sh\n").unwrap();
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked_in().once().returning(|_, _, _| {
            Ok(ExecResult {
                stdout: String::new(),
                stderr: "syntax error".to_string(),
                success: false,
                code: Some(2),
            })
        });
        let resource = ScriptResource::new(
            "test".to_string(),
            script_path,
            dir.path().to_path_buf(),
            Arc::new(mock),
        );
        let state = resource.current_state().unwrap();
        assert!(matches!(
            state,
            ResourceState::Unknown { reason } if reason.contains("exit 2") && reason.contains("syntax error")
        ));
    }
}
