//! Login shell configuration resource.
use std::sync::Arc;

use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::exec::{CommandSpec, Executor};

/// Source for reading the current login shell.
///
/// Reads `SHELL` through the injected environment handle so tests can supply
/// a deterministic value without `unsafe` env-var manipulation.
#[derive(Debug, Clone)]
struct ShellSource(Arc<dyn crate::infra::env::Env>);

impl ShellSource {
    /// Return the current shell value.
    fn current_shell(&self) -> Option<String> {
        self.0.var("SHELL")
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
    pub fn new(
        target_shell: String,
        executor: Arc<dyn Executor>,
        env: Arc<dyn crate::infra::env::Env>,
    ) -> Self {
        Self {
            target_shell,
            executor,
            shell_source: ShellSource(env),
        }
    }

    /// Override the shell source with a fixed value (for testing).
    #[cfg(test)]
    #[must_use]
    fn with_shell(mut self, shell: Option<&str>) -> Self {
        let mut env = crate::infra::env::MapEnv::new();
        if let Some(shell) = shell {
            env = env.with("SHELL", shell);
        }
        self.shell_source = ShellSource(env.into_handle());
        self
    }
}

impl Resource for DefaultShellResource {
    fn description(&self) -> String {
        format!("default shell → {}", self.target_shell)
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        let shell_path = self.executor.which_path(&self.target_shell)?;
        let shell_str = shell_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 shell path: {}", shell_path.display()))?;
        self.executor
            .execute(CommandSpec::new("chsh").args(&["-s", shell_str]))?;
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for DefaultShellResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        let Some(current_shell) = self.shell_source.current_shell() else {
            return Ok(ResourceState::Unknown {
                reason: "SHELL environment variable is not set".into(),
            });
        };

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
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::infra::exec::{ExecError, ExecResult, MockExecutor};
    use std::path::PathBuf;

    fn ok_result() -> ExecResult {
        ExecResult {
            stdout: String::new(),
            stderr: String::new(),
            success: true,
            code: Some(0),
        }
    }

    #[test]
    fn description_includes_shell_name() {
        let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::ProcessExecutor::system());
        let resource = DefaultShellResource::new(
            "zsh".to_string(),
            Arc::clone(&executor),
            crate::infra::env::MapEnv::new().into_handle(),
        );
        assert_eq!(resource.description(), "default shell → zsh");
    }

    #[test]
    fn current_state_correct_when_shell_matches() {
        let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::ProcessExecutor::system());
        let resource = DefaultShellResource::new(
            "zsh".to_string(),
            Arc::clone(&executor),
            crate::infra::env::MapEnv::new().into_handle(),
        )
        .with_shell(Some("/usr/bin/zsh"));
        let state = resource.current_state().unwrap();
        assert_eq!(state, ResourceState::Correct);
    }

    #[test]
    fn current_state_incorrect_when_different_shell_set() {
        let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::ProcessExecutor::system());
        let resource = DefaultShellResource::new(
            "zsh".to_string(),
            Arc::clone(&executor),
            crate::infra::env::MapEnv::new().into_handle(),
        )
        .with_shell(Some("/bin/bash"));
        let state = resource.current_state().unwrap();
        assert!(
            matches!(state, ResourceState::Incorrect { ref current } if current == "/bin/bash"),
            "expected Incorrect(/bin/bash), got {state:?}"
        );
    }

    #[test]
    fn current_state_unknown_when_shell_not_set() {
        let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::ProcessExecutor::system());
        let resource = DefaultShellResource::new(
            "zsh".to_string(),
            Arc::clone(&executor),
            crate::infra::env::MapEnv::new().into_handle(),
        )
        .with_shell(None);
        let state = resource.current_state().unwrap();
        assert!(
            matches!(state, ResourceState::Unknown { ref reason } if reason.contains("SHELL")),
            "expected Unknown(SHELL ...), got {state:?}"
        );
    }

    #[test]
    fn current_state_missing_when_shell_is_empty_string() {
        let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::ProcessExecutor::system());
        let resource = DefaultShellResource::new(
            "zsh".to_string(),
            Arc::clone(&executor),
            crate::infra::env::MapEnv::new().into_handle(),
        )
        .with_shell(Some(""));
        let state = resource.current_state().unwrap();
        assert_eq!(state, ResourceState::Missing);
    }

    #[test]
    fn apply_runs_chsh_with_the_resolved_shell_path() {
        let mut mock = MockExecutor::new();
        mock.expect_which_path()
            .once()
            .returning(|program| Ok(PathBuf::from(format!("/usr/bin/{program}"))));
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "chsh"
                    && spec.arguments() == ["-s", "/usr/bin/zsh"]
                    && spec.is_checked()
            })
            .returning(|_| Ok(ok_result()));

        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = DefaultShellResource::new(
            "zsh".to_string(),
            executor,
            crate::infra::env::MapEnv::new().into_handle(),
        );
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[test]
    fn apply_fails_without_running_chsh_when_shell_is_not_on_path() {
        let mut mock = MockExecutor::new();
        mock.expect_which_path()
            .once()
            .returning(|program| Err(anyhow::anyhow!("{program} not found on PATH")));
        mock.expect_execute().never();

        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = DefaultShellResource::new(
            "zsh".to_string(),
            executor,
            crate::infra::env::MapEnv::new().into_handle(),
        );
        assert!(
            resource.apply().is_err(),
            "apply should fail when the target shell cannot be resolved"
        );
    }

    #[test]
    fn apply_propagates_chsh_failure() {
        let mut mock = MockExecutor::new();
        mock.expect_which_path()
            .once()
            .returning(|_| Ok(PathBuf::from("/usr/bin/zsh")));
        mock.expect_execute().once().returning(|_| {
            Err(ExecError::spawn(
                "chsh",
                std::io::Error::other("PAM authentication failed"),
            ))
        });

        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = DefaultShellResource::new(
            "zsh".to_string(),
            executor,
            crate::infra::env::MapEnv::new().into_handle(),
        );
        assert!(
            resource.apply().is_err(),
            "apply should surface a failing chsh invocation"
        );
    }
}
