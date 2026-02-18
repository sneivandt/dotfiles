use std::collections::HashSet;

use anyhow::Result;

use super::{Resource, ResourceChange, ResourceState};
use crate::exec;

/// A VS Code extension resource that can be checked and installed.
#[derive(Debug, Clone)]
pub struct VsCodeExtensionResource {
    /// Extension identifier (e.g. "github.copilot-chat").
    pub id: String,
    /// VS Code CLI command to use (e.g. "code-insiders" or "code").
    pub code_cmd: String,
}

impl VsCodeExtensionResource {
    /// Create a new VS Code extension resource.
    #[must_use]
    pub const fn new(id: String, code_cmd: String) -> Self {
        Self { id, code_cmd }
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
pub fn get_installed_extensions(code_cmd: &str) -> Result<HashSet<String>> {
    let result = run_code_cmd(code_cmd, &["--list-extensions"])?;
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

impl Resource for VsCodeExtensionResource {
    fn description(&self) -> String {
        self.id.clone()
    }

    fn current_state(&self) -> Result<ResourceState> {
        let result = run_code_cmd(&self.code_cmd, &["--list-extensions"])?;
        let installed = result.stdout.to_lowercase();
        if installed
            .lines()
            .any(|l| l.trim() == self.id.to_lowercase())
        {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Missing)
        }
    }

    fn apply(&self) -> Result<ResourceChange> {
        let result = run_code_cmd(
            &self.code_cmd,
            &["--install-extension", &self.id, "--force"],
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
pub fn find_code_command() -> Option<String> {
    for cmd in &["code-insiders", "code"] {
        if exec::which(cmd) {
            return Some((*cmd).to_string());
        }
    }
    None
}

/// Run a VS Code CLI command. On Windows, `.cmd` wrappers need `cmd.exe /C`.
fn run_code_cmd(cmd: &str, args: &[&str]) -> Result<exec::ExecResult> {
    #[cfg(target_os = "windows")]
    {
        let mut full_args = vec!["/C", cmd];
        full_args.extend(args);
        exec::run_unchecked("cmd", &full_args)
    }

    #[cfg(not(target_os = "windows"))]
    {
        exec::run_unchecked(cmd, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn description_returns_extension_id() {
        let resource =
            VsCodeExtensionResource::new("github.copilot-chat".to_string(), "code".to_string());
        assert_eq!(resource.description(), "github.copilot-chat");
    }

    #[test]
    fn state_from_installed_correct() {
        let resource =
            VsCodeExtensionResource::new("github.copilot-chat".to_string(), "code".to_string());
        let mut installed = HashSet::new();
        installed.insert("github.copilot-chat".to_string());
        assert_eq!(
            resource.state_from_installed(&installed),
            ResourceState::Correct
        );
    }

    #[test]
    fn state_from_installed_case_insensitive() {
        let resource =
            VsCodeExtensionResource::new("GitHub.Copilot-Chat".to_string(), "code".to_string());
        let mut installed = HashSet::new();
        installed.insert("github.copilot-chat".to_string()); // lowercase in set
        assert_eq!(
            resource.state_from_installed(&installed),
            ResourceState::Correct
        );
    }

    #[test]
    fn state_from_installed_missing() {
        let resource =
            VsCodeExtensionResource::new("github.copilot-chat".to_string(), "code".to_string());
        let installed = HashSet::new();
        assert_eq!(
            resource.state_from_installed(&installed),
            ResourceState::Missing
        );
    }
}
