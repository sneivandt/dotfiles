//! Overlay script resource.
//!
//! Runs custom scripts from a private overlay repository.  Scripts follow a
//! convention-based interface:
//!
//! - **Check**: Run the script with `--check`.  Exit code 0 means the resource
//!   is in the correct state; non-zero means it needs to be applied.
//! - **Apply**: Run the script with no arguments to apply the desired state.
//! - **Remove**: Run the script with `--remove` to undo the applied state.
//!
//! `PowerShell` scripts (`.ps1`) are invoked via `pwsh`/`powershell`, shell
//! scripts (`.sh`) are invoked via `sh`.
use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Applicable, Resource, ResourceChange, ResourceState};
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
    #[must_use]
    pub fn from_entry(
        entry: &crate::config::scripts::ScriptEntry,
        overlay_root: &Path,
        executor: Arc<dyn Executor>,
    ) -> Self {
        let script_path = crate::config::scripts::resolve_script_path(entry, overlay_root);
        Self::new(
            entry.name.clone(),
            script_path,
            overlay_root.to_path_buf(),
            executor,
        )
    }

    /// Determine the interpreter and arguments for the script based on its extension.
    fn interpreter_args(&self) -> (&str, Vec<&str>) {
        let ext = self
            .script_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        match ext {
            "ps1" => {
                let shell = if cfg!(windows) { "powershell" } else { "pwsh" };
                (
                    shell,
                    vec![
                        "-NoProfile",
                        "-NonInteractive",
                        "-ExecutionPolicy",
                        "Bypass",
                        "-File",
                    ],
                )
            }
            _ => ("sh", vec![]),
        }
    }

    /// Run the script (apply) and return the change along with captured stdout.
    ///
    /// Use this instead of [`Applicable::apply`] when you want to forward the
    /// script's per-item log lines through the engine logger.
    ///
    /// # Errors
    ///
    /// Returns an error if the script fails to execute or exits with a
    /// non-zero status code.
    pub fn apply_verbose(&self) -> Result<(ResourceChange, String)> {
        if !self.script_path.exists() {
            return Ok((
                ResourceChange::Skipped {
                    reason: format!("script not found: {}", self.script_path.display()),
                },
                String::new(),
            ));
        }

        let (interpreter, mut args) = self.interpreter_args();
        let script_str = self.script_path.display().to_string();
        args.push(&script_str);

        let args_refs: Vec<&str> = args.clone();
        let result = self
            .executor
            .run_in(&self.working_dir, interpreter, &args_refs)
            .with_context(|| format!("running script: {}", self.name))?;

        Ok((ResourceChange::Applied, result.stdout))
    }

    /// Run the script in dry-run mode and return captured stdout.
    ///
    /// Passes `--dryrun` to the script, which should print what it would do
    /// without making changes.
    ///
    /// # Errors
    ///
    /// Returns an error if the script fails to execute.
    pub fn dry_run_output(&self) -> Result<String> {
        if !self.script_path.exists() {
            return Ok(String::new());
        }

        let (interpreter, mut args) = self.interpreter_args();
        let script_str = self.script_path.display().to_string();
        args.push(&script_str);
        args.push("--dryrun");

        let args_refs: Vec<&str> = args.clone();
        let result = self
            .executor
            .run_unchecked(interpreter, &args_refs)
            .with_context(|| format!("dry-run script: {}", self.name))?;

        Ok(result.stdout)
    }
}

impl Applicable for ScriptResource {
    fn description(&self) -> String {
        self.name.clone()
    }

    fn apply(&self) -> Result<ResourceChange> {
        if !self.script_path.exists() {
            return Ok(ResourceChange::Skipped {
                reason: format!("script not found: {}", self.script_path.display()),
            });
        }

        let (interpreter, mut args) = self.interpreter_args();
        let script_str = self.script_path.display().to_string();
        args.push(&script_str);

        let args_refs: Vec<&str> = args.clone();
        self.executor
            .run_in(&self.working_dir, interpreter, &args_refs)
            .with_context(|| format!("running script: {}", self.name))?;

        Ok(ResourceChange::Applied)
    }

    fn remove(&self) -> Result<ResourceChange> {
        if !self.script_path.exists() {
            return Ok(ResourceChange::Skipped {
                reason: format!("script not found: {}", self.script_path.display()),
            });
        }

        let (interpreter, mut args) = self.interpreter_args();
        let script_str = self.script_path.display().to_string();
        args.push(&script_str);
        args.push("--remove");

        let args_refs: Vec<&str> = args.clone();
        self.executor
            .run_in(&self.working_dir, interpreter, &args_refs)
            .with_context(|| format!("removing script: {}", self.name))?;

        Ok(ResourceChange::Applied)
    }
}

impl Resource for ScriptResource {
    fn current_state(&self) -> Result<ResourceState> {
        if !self.script_path.exists() {
            return Ok(ResourceState::Invalid {
                reason: format!("script not found: {}", self.script_path.display()),
            });
        }

        let (interpreter, mut args) = self.interpreter_args();
        let script_str = self.script_path.display().to_string();
        args.push(&script_str);
        args.push("--check");

        let args_refs: Vec<&str> = args.clone();
        let result = self
            .executor
            .run_unchecked(interpreter, &args_refs)
            .with_context(|| format!("checking script state: {}", self.name))?;

        if result.success {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Missing)
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::exec::MockExecutor;

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
        let (interpreter, _) = resource.interpreter_args();
        assert_eq!(interpreter, "sh");
    }

    #[test]
    fn interpreter_uses_powershell_for_ps1_scripts() {
        let mock = Arc::new(MockExecutor::new());
        let resource = make_script_resource("test", Path::new("/scripts/test.ps1"), mock);
        let (interpreter, args) = resource.interpreter_args();
        if cfg!(windows) {
            assert_eq!(interpreter, "powershell");
        } else {
            assert_eq!(interpreter, "pwsh");
        }
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
        let resource = ScriptResource::from_entry(&entry, Path::new("/overlay"), mock);
        assert_eq!(
            resource.script_path,
            PathBuf::from("/overlay/scripts/setup-db.ps1")
        );
        assert_eq!(resource.working_dir, PathBuf::from("/overlay"));
    }
}
