//! Login shell configuration resource.
use anyhow::Result;
use std::sync::Arc;

use super::{Applicable, Resource, ResourceChange, ResourceState};
use crate::exec::Executor;

/// Source for reading the current login shell.
///
/// Production code uses [`ShellSource::Environment`] to read the `SHELL`
/// environment variable at check time.  Tests use [`ShellSource::Fixed`]
/// to inject a deterministic value without `unsafe` env-var manipulation.
#[derive(Debug, Clone)]
enum ShellSource {
    /// Read from the `SHELL` environment variable at check time.
    Environment,
    /// Use a fixed value (for testing).
    #[cfg(test)]
    Fixed(Option<String>),
}

impl ShellSource {
    /// Return the current shell value.
    fn current_shell(&self) -> Option<String> {
        match self {
            Self::Environment => std::env::var("SHELL").ok(),
            #[cfg(test)]
            Self::Fixed(value) => value.clone(),
        }
    }
}

/// A resource for configuring the default login shell.
#[derive(Debug)]
pub struct DefaultShellResource {
    /// Target shell name (e.g., "zsh").
    target_shell: String,
    /// Executor for running system commands.
    executor: Arc<dyn Executor>,
    /// Source for the current shell value.
    shell_source: ShellSource,
}

impl DefaultShellResource {
    /// Create a new default shell resource.
    #[must_use]
    pub fn new(target_shell: String, executor: Arc<dyn Executor>) -> Self {
        Self {
            target_shell,
            executor,
            shell_source: ShellSource::Environment,
        }
    }

    /// Override the shell source with a fixed value (for testing).
    #[cfg(test)]
    #[must_use]
    fn with_shell(mut self, shell: Option<&str>) -> Self {
        self.shell_source = ShellSource::Fixed(shell.map(String::from));
        self
    }
}

impl Applicable for DefaultShellResource {
    fn description(&self) -> String {
        format!("default shell → {}", self.target_shell)
    }

    fn apply(&self) -> Result<ResourceChange> {
        let shell_path = self.executor.which_path(&self.target_shell)?;
        let shell_str = shell_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 shell path: {}", shell_path.display()))?;
        self.executor.run("chsh", &["-s", shell_str])?;
        Ok(ResourceChange::Applied)
    }
}

impl Resource for DefaultShellResource {
    fn current_state(&self) -> Result<ResourceState> {
        let current_shell = self.shell_source.current_shell().unwrap_or_default();

        if current_shell.is_empty() {
            return Ok(ResourceState::Missing);
        }

        let current_name = std::path::Path::new(&current_shell)
            .file_name()
            .and_then(|n| n.to_str());

        if current_name == Some(&self.target_shell) {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Incorrect {
                current: current_shell,
            })
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn description_includes_shell_name() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = DefaultShellResource::new("zsh".to_string(), Arc::clone(&executor));
        assert_eq!(resource.description(), "default shell → zsh");
    }

    #[test]
    fn current_state_correct_when_shell_matches() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = DefaultShellResource::new("zsh".to_string(), Arc::clone(&executor))
            .with_shell(Some("/usr/bin/zsh"));
        let state = resource.current_state().unwrap();
        assert_eq!(state, ResourceState::Correct);
    }

    #[test]
    fn current_state_incorrect_when_different_shell_set() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = DefaultShellResource::new("zsh".to_string(), Arc::clone(&executor))
            .with_shell(Some("/bin/bash"));
        let state = resource.current_state().unwrap();
        assert!(
            matches!(state, ResourceState::Incorrect { ref current } if current == "/bin/bash"),
            "expected Incorrect(/bin/bash), got {state:?}"
        );
    }

    #[test]
    fn current_state_missing_when_shell_not_set() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource =
            DefaultShellResource::new("zsh".to_string(), Arc::clone(&executor)).with_shell(None);
        let state = resource.current_state().unwrap();
        assert_eq!(state, ResourceState::Missing);
    }
}
