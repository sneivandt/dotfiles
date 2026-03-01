//! VS Code extension resource.
use std::collections::HashSet;

use anyhow::Result;

use super::{Resource, ResourceChange, ResourceState};
use crate::exec::{self, Executor};

/// A VS Code extension resource that can be checked and installed.
#[derive(Debug)]
pub struct VsCodeExtensionResource<'a> {
    /// Extension identifier (e.g. "github.copilot-chat").
    pub id: String,
    /// VS Code CLI command to use (e.g. "code-insiders" or "code").
    pub code_cmd: String,
    /// Executor for running VS Code CLI commands.
    executor: &'a dyn Executor,
}

impl<'a> VsCodeExtensionResource<'a> {
    /// Create a new VS Code extension resource.
    #[must_use]
    pub const fn new(id: String, code_cmd: String, executor: &'a dyn Executor) -> Self {
        Self {
            id,
            code_cmd,
            executor,
        }
    }

    /// Determine the resource state from a pre-fetched set of installed extension IDs.
    ///
    /// This avoids running `code --list-extensions` per resource when used
    /// with [`get_installed_extensions`].
    #[must_use]
    pub fn state_from_installed(&self, installed: &HashSet<String>) -> ResourceState {
        if installed.contains(&self.id.to_lowercase()) {
            ResourceState::Correct
        } else {
            ResourceState::Missing
        }
    }
}

/// Query the full set of installed VS Code extension IDs in a single command.
///
/// Returns a `HashSet` of **lower-cased** extension IDs.
///
/// # Errors
///
/// Returns an error if the VS Code command fails to execute, cannot be found,
/// or exits with a non-zero status code.
pub fn get_installed_extensions(
    code_cmd: &str,
    executor: &dyn Executor,
) -> Result<HashSet<String>> {
    let result = run_code_cmd(code_cmd, &["--list-extensions"], executor)?;
    let mut set = HashSet::new();
    if result.success {
        for line in result.stdout.lines() {
            let id = line.trim().to_lowercase();
            if !id.is_empty() {
                set.insert(id);
            }
        }
    }
    Ok(set)
}

impl Resource for VsCodeExtensionResource<'_> {
    fn description(&self) -> String {
        self.id.clone()
    }

    /// Check whether this extension is installed.
    ///
    /// **Performance note**: This method runs `code --list-extensions` as a
    /// subprocess every time it is called. When checking multiple extensions,
    /// prefer calling [`get_installed_extensions`] once and using
    /// [`Self::state_from_installed`] on each resource to avoid N subprocess
    /// invocations.
    fn current_state(&self) -> Result<ResourceState> {
        let installed = get_installed_extensions(&self.code_cmd, self.executor)?;
        Ok(self.state_from_installed(&installed))
    }

    fn apply(&self) -> Result<ResourceChange> {
        let result = run_code_cmd(
            &self.code_cmd,
            &["--install-extension", &self.id, "--force"],
            self.executor,
        )?;
        if result.success {
            Ok(ResourceChange::Applied)
        } else {
            Ok(ResourceChange::Skipped {
                reason: format!("failed to install: {}", result.stderr.trim()),
            })
        }
    }
}

/// Find the VS Code CLI command, preferring code-insiders.
#[must_use]
pub fn find_code_command(executor: &dyn Executor) -> Option<String> {
    for cmd in ["code-insiders", "code"] {
        if executor.which(cmd) {
            return Some(cmd.to_string());
        }
    }
    None
}

/// Run a VS Code CLI command. On Windows, `.cmd` wrappers need `cmd.exe /C`.
///
/// # Errors
///
/// Returns an error if the command execution fails or if the command cannot be found.
fn run_code_cmd(cmd: &str, args: &[&str], executor: &dyn Executor) -> Result<exec::ExecResult> {
    #[cfg(target_os = "windows")]
    {
        let mut full_args = vec!["/C", cmd];
        full_args.extend(args);
        executor.run_unchecked("cmd", &full_args)
    }

    #[cfg(not(target_os = "windows"))]
    {
        executor.run_unchecked(cmd, args)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::resources::test_helpers::MockExecutor;

    #[test]
    fn description_returns_extension_id() {
        let executor = crate::exec::SystemExecutor;
        let resource = VsCodeExtensionResource::new(
            "github.copilot-chat".to_string(),
            "code".to_string(),
            &executor,
        );
        assert_eq!(resource.description(), "github.copilot-chat");
    }

    #[test]
    fn state_from_installed_correct() {
        let executor = crate::exec::SystemExecutor;
        let resource = VsCodeExtensionResource::new(
            "github.copilot-chat".to_string(),
            "code".to_string(),
            &executor,
        );
        let mut installed = HashSet::new();
        installed.insert("github.copilot-chat".to_string());
        assert_eq!(
            resource.state_from_installed(&installed),
            ResourceState::Correct
        );
    }

    #[test]
    fn state_from_installed_case_insensitive() {
        let executor = crate::exec::SystemExecutor;
        let resource = VsCodeExtensionResource::new(
            "GitHub.Copilot-Chat".to_string(),
            "code".to_string(),
            &executor,
        );
        let mut installed = HashSet::new();
        installed.insert("github.copilot-chat".to_string()); // lowercase in set
        assert_eq!(
            resource.state_from_installed(&installed),
            ResourceState::Correct
        );
    }

    #[test]
    fn state_from_installed_missing() {
        let executor = crate::exec::SystemExecutor;
        let resource = VsCodeExtensionResource::new(
            "github.copilot-chat".to_string(),
            "code".to_string(),
            &executor,
        );
        let installed = HashSet::new();
        assert_eq!(
            resource.state_from_installed(&installed),
            ResourceState::Missing
        );
    }

    // ------------------------------------------------------------------
    // get_installed_extensions
    // ------------------------------------------------------------------

    #[test]
    fn get_installed_extensions_parses_and_lowercases() {
        let executor =
            MockExecutor::ok("GitHub.Copilot\nms-python.python\nRust-lang.Rust-analyzer\n");
        let installed = get_installed_extensions("code", &executor).unwrap();
        assert!(installed.contains("github.copilot"));
        assert!(installed.contains("ms-python.python"));
        assert!(installed.contains("rust-lang.rust-analyzer"));
    }

    #[test]
    fn get_installed_extensions_empty_when_command_fails() {
        let executor = MockExecutor::fail();
        let installed = get_installed_extensions("code", &executor).unwrap();
        assert!(installed.is_empty());
    }

    #[test]
    fn current_state_calls_executor_once() {
        let executor = MockExecutor::ok("github.copilot-chat\n");
        let resource = VsCodeExtensionResource::new(
            "github.copilot-chat".to_string(),
            "code".to_string(),
            &executor,
        );
        let state = resource.current_state().unwrap();
        assert_eq!(state, ResourceState::Correct);
        assert_eq!(
            executor.call_count(),
            1,
            "current_state should call the executor exactly once"
        );
    }
}
