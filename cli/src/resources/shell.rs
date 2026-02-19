use anyhow::Result;

use super::{Resource, ResourceChange, ResourceState};
use crate::exec::Executor;

/// A resource for configuring the default login shell.
#[derive(Debug)]
pub struct DefaultShellResource<'a> {
    /// Target shell name (e.g., "zsh").
    target_shell: String,
    /// Executor for running system commands.
    executor: &'a dyn Executor,
}

impl<'a> DefaultShellResource<'a> {
    /// Create a new default shell resource.
    #[must_use]
    pub fn new(target_shell: String, executor: &'a dyn Executor) -> Self {
        Self {
            target_shell,
            executor,
        }
    }
}

impl Resource for DefaultShellResource<'_> {
    fn description(&self) -> String {
        format!("default shell → {}", self.target_shell)
    }

    fn current_state(&self) -> Result<ResourceState> {
        let current_shell = std::env::var("SHELL").unwrap_or_default();
        let suffix = format!("/{}", self.target_shell);

        if current_shell.ends_with(&suffix) {
            Ok(ResourceState::Correct)
        } else if current_shell.is_empty() {
            Ok(ResourceState::Missing)
        } else {
            Ok(ResourceState::Incorrect {
                current: current_shell,
            })
        }
    }

    fn apply(&self) -> Result<ResourceChange> {
        let result = self.executor.run("which", &[&self.target_shell])?;
        let shell_path = result.stdout.trim();
        self.executor.run("chsh", &["-s", shell_path])?;
        Ok(ResourceChange::Applied)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn description_includes_shell_name() {
        let executor = crate::exec::SystemExecutor;
        let resource = DefaultShellResource::new("zsh".to_string(), &executor);
        assert_eq!(resource.description(), "default shell → zsh");
    }
}
