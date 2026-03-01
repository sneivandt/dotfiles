//! Login shell configuration resource.
use anyhow::Result;
use std::sync::Arc;

use super::{Applicable, Resource, ResourceChange, ResourceState};
use crate::exec::Executor;

/// A resource for configuring the default login shell.
#[derive(Debug)]
pub struct DefaultShellResource {
    /// Target shell name (e.g., "zsh").
    target_shell: String,
    /// Executor for running system commands.
    executor: Arc<dyn Executor>,
}

impl DefaultShellResource {
    /// Create a new default shell resource.
    #[must_use]
    pub fn new(target_shell: String, executor: Arc<dyn Executor>) -> Self {
        Self {
            target_shell,
            executor,
        }
    }
}

impl Applicable for DefaultShellResource {
    fn description(&self) -> String {
        format!("default shell → {}", self.target_shell)
    }

    fn apply(&self) -> Result<ResourceChange> {
        let shell_path = crate::exec::which_path(&self.target_shell)?;
        let shell_str = shell_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 shell path: {}", shell_path.display()))?;
        self.executor.run("chsh", &["-s", shell_str])?;
        Ok(ResourceChange::Applied)
    }
}

impl Resource for DefaultShellResource {
    fn current_state(&self) -> Result<ResourceState> {
        let current_shell = std::env::var("SHELL").unwrap_or_default();

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
#[allow(unsafe_code)] // set_var/remove_var require unsafe since Rust 1.83
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Mutex to serialize tests that mutate the `SHELL` environment variable.
    /// Without this, tests running in parallel threads race on the same env var.
    static SHELL_MUTEX: std::sync::LazyLock<std::sync::Mutex<()>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(()));

    #[test]
    fn description_includes_shell_name() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = DefaultShellResource::new("zsh".to_string(), Arc::clone(&executor));
        assert_eq!(resource.description(), "default shell → zsh");
    }

    #[test]
    fn current_state_correct_when_shell_matches() {
        let _guard = SHELL_MUTEX.lock().expect("mutex poisoned");
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = DefaultShellResource::new("zsh".to_string(), Arc::clone(&executor));
        // SAFETY: test-only env var manipulation; serialized via SHELL_MUTEX.
        unsafe { std::env::set_var("SHELL", "/usr/bin/zsh") };
        let state = resource.current_state().unwrap();
        unsafe { std::env::remove_var("SHELL") };
        assert_eq!(state, ResourceState::Correct);
    }

    #[test]
    fn current_state_incorrect_when_different_shell_set() {
        let _guard = SHELL_MUTEX.lock().expect("mutex poisoned");
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = DefaultShellResource::new("zsh".to_string(), Arc::clone(&executor));
        // SAFETY: test-only env var manipulation; serialized via SHELL_MUTEX.
        unsafe { std::env::set_var("SHELL", "/bin/bash") };
        let state = resource.current_state().unwrap();
        unsafe { std::env::remove_var("SHELL") };
        assert!(
            matches!(state, ResourceState::Incorrect { ref current } if current == "/bin/bash"),
            "expected Incorrect(/bin/bash), got {state:?}"
        );
    }

    #[test]
    fn current_state_missing_when_shell_not_set() {
        let _guard = SHELL_MUTEX.lock().expect("mutex poisoned");
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = DefaultShellResource::new("zsh".to_string(), Arc::clone(&executor));
        // SAFETY: test-only env var manipulation; serialized via SHELL_MUTEX.
        unsafe { std::env::remove_var("SHELL") };
        let state = resource.current_state().unwrap();
        assert_eq!(state, ResourceState::Missing);
    }
}
