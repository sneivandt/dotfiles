use anyhow::Result;

use super::{Resource, ResourceChange, ResourceState};
use crate::exec::Executor;

/// A git config entry resource that can be checked and applied.
#[derive(Debug)]
pub struct GitConfigResource<'a> {
    /// Config key (e.g., "core.autocrlf").
    pub key: String,
    /// Desired value (e.g., "false").
    pub desired_value: String,
    /// Executor for running git commands.
    executor: &'a dyn Executor,
}

impl<'a> GitConfigResource<'a> {
    /// Create a new git config resource.
    #[must_use]
    pub fn new(key: String, desired_value: String, executor: &'a dyn Executor) -> Self {
        Self {
            key,
            desired_value,
            executor,
        }
    }
}

impl Resource for GitConfigResource<'_> {
    fn description(&self) -> String {
        format!("{} = {}", self.key, self.desired_value)
    }

    fn current_state(&self) -> Result<ResourceState> {
        let result = self
            .executor
            .run_unchecked("git", &["config", "--global", "--get", &self.key])?;
        let current = result.stdout.trim().to_string();

        if !result.success || current.is_empty() {
            Ok(ResourceState::Missing)
        } else if current == self.desired_value {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Incorrect { current })
        }
    }

    fn apply(&self) -> Result<ResourceChange> {
        self.executor.run(
            "git",
            &["config", "--global", &self.key, &self.desired_value],
        )?;
        Ok(ResourceChange::Applied)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn description_format() {
        let executor = crate::exec::SystemExecutor;
        let resource =
            GitConfigResource::new("core.autocrlf".to_string(), "false".to_string(), &executor);
        assert_eq!(resource.description(), "core.autocrlf = false");
    }

    // ------------------------------------------------------------------
    // MockExecutor
    // ------------------------------------------------------------------

    #[derive(Debug)]
    struct MockExecutor {
        responses: std::cell::RefCell<std::collections::VecDeque<(bool, String)>>,
    }

    impl MockExecutor {
        fn ok(stdout: &str) -> Self {
            Self {
                responses: std::cell::RefCell::new(std::collections::VecDeque::from([(
                    true,
                    stdout.to_string(),
                )])),
            }
        }

        fn fail() -> Self {
            Self {
                responses: std::cell::RefCell::new(std::collections::VecDeque::from([(
                    false,
                    String::new(),
                )])),
            }
        }

        fn next(&self) -> (bool, String) {
            self.responses
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| (false, "unexpected call".to_string()))
        }
    }

    impl crate::exec::Executor for MockExecutor {
        fn run(&self, _: &str, _: &[&str]) -> anyhow::Result<crate::exec::ExecResult> {
            let (success, stdout) = self.next();
            if success {
                Ok(crate::exec::ExecResult {
                    stdout,
                    stderr: String::new(),
                    success: true,
                    code: Some(0),
                })
            } else {
                anyhow::bail!("mock command failed")
            }
        }

        fn run_in(
            &self,
            _: &std::path::Path,
            _: &str,
            _: &[&str],
        ) -> anyhow::Result<crate::exec::ExecResult> {
            let (success, stdout) = self.next();
            if success {
                Ok(crate::exec::ExecResult {
                    stdout,
                    stderr: String::new(),
                    success: true,
                    code: Some(0),
                })
            } else {
                anyhow::bail!("mock command failed")
            }
        }

        fn run_in_with_env(
            &self,
            _: &std::path::Path,
            _: &str,
            _: &[&str],
            _: &[(&str, &str)],
        ) -> anyhow::Result<crate::exec::ExecResult> {
            let (success, stdout) = self.next();
            if success {
                Ok(crate::exec::ExecResult {
                    stdout,
                    stderr: String::new(),
                    success: true,
                    code: Some(0),
                })
            } else {
                anyhow::bail!("mock command failed")
            }
        }

        fn run_unchecked(&self, _: &str, _: &[&str]) -> anyhow::Result<crate::exec::ExecResult> {
            let (success, stdout) = self.next();
            Ok(crate::exec::ExecResult {
                stdout,
                stderr: String::new(),
                success,
                code: Some(i32::from(!success)),
            })
        }

        fn which(&self, _: &str) -> bool {
            false
        }
    }

    // ------------------------------------------------------------------
    // current_state
    // ------------------------------------------------------------------

    #[test]
    fn current_state_correct_when_value_matches() {
        let executor = MockExecutor::ok("false\n");
        let resource =
            GitConfigResource::new("core.autocrlf".to_string(), "false".to_string(), &executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_missing_when_command_fails() {
        let executor = MockExecutor::fail();
        let resource =
            GitConfigResource::new("core.autocrlf".to_string(), "false".to_string(), &executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    #[test]
    fn current_state_missing_when_output_empty() {
        let executor = MockExecutor::ok("");
        let resource =
            GitConfigResource::new("core.autocrlf".to_string(), "false".to_string(), &executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    #[test]
    fn current_state_incorrect_when_value_differs() {
        let executor = MockExecutor::ok("true\n");
        let resource =
            GitConfigResource::new("core.autocrlf".to_string(), "false".to_string(), &executor);
        let state = resource.current_state().unwrap();
        assert!(
            matches!(state, ResourceState::Incorrect { ref current } if current == "true"),
            "expected Incorrect(true), got {state:?}"
        );
    }

    // ------------------------------------------------------------------
    // apply
    // ------------------------------------------------------------------

    #[test]
    fn apply_returns_applied_on_success() {
        let executor = MockExecutor::ok("");
        let resource =
            GitConfigResource::new("core.autocrlf".to_string(), "false".to_string(), &executor);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }
}
