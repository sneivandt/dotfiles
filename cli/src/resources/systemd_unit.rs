use anyhow::Result;

use super::{Resource, ResourceChange, ResourceState};
use crate::exec::Executor;

/// A systemd user unit resource that can be checked and enabled.
#[derive(Debug)]
pub struct SystemdUnitResource<'a> {
    /// Unit name (e.g. "clean-home-tmp.timer").
    pub name: String,
    /// Executor for running systemctl commands.
    executor: &'a dyn Executor,
}

impl<'a> SystemdUnitResource<'a> {
    /// Create a new systemd unit resource.
    #[must_use]
    pub const fn new(name: String, executor: &'a dyn Executor) -> Self {
        Self { name, executor }
    }

    /// Create from a config entry.
    #[must_use]
    pub fn from_entry(
        entry: &crate::config::systemd_units::SystemdUnit,
        executor: &'a dyn Executor,
    ) -> Self {
        Self::new(entry.name.clone(), executor)
    }
}

impl Resource for SystemdUnitResource<'_> {
    fn description(&self) -> String {
        self.name.clone()
    }

    fn current_state(&self) -> Result<ResourceState> {
        let result = self
            .executor
            .run_unchecked("systemctl", &["--user", "is-enabled", &self.name])?;
        if result.success {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Missing)
        }
    }

    fn apply(&self) -> Result<ResourceChange> {
        let result = self
            .executor
            .run_unchecked("systemctl", &["--user", "enable", "--now", &self.name])?;
        if result.success {
            Ok(ResourceChange::Applied)
        } else {
            Ok(ResourceChange::Skipped {
                reason: format!("failed to enable: {}", result.stderr.trim()),
            })
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn description_returns_unit_name() {
        let executor = crate::exec::SystemExecutor;
        let resource = SystemdUnitResource::new("clean-home-tmp.timer".to_string(), &executor);
        assert_eq!(resource.description(), "clean-home-tmp.timer");
    }

    #[test]
    fn from_entry_copies_name() {
        let executor = crate::exec::SystemExecutor;
        let entry = crate::config::systemd_units::SystemdUnit {
            name: "dunst.service".to_string(),
        };
        let resource = SystemdUnitResource::from_entry(&entry, &executor);
        assert_eq!(resource.name, "dunst.service");
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
                .unwrap_or((false, "unexpected call".to_string()))
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
                code: Some(if success { 0 } else { 1 }),
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
    fn current_state_correct_when_systemctl_reports_enabled() {
        let executor = MockExecutor::ok("enabled\n");
        let resource = SystemdUnitResource::new("dunst.service".to_string(), &executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_missing_when_systemctl_fails() {
        let executor = MockExecutor::fail();
        let resource = SystemdUnitResource::new("dunst.service".to_string(), &executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    // ------------------------------------------------------------------
    // apply
    // ------------------------------------------------------------------

    #[test]
    fn apply_returns_applied_when_systemctl_succeeds() {
        let executor = MockExecutor::ok("");
        let resource = SystemdUnitResource::new("dunst.service".to_string(), &executor);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[test]
    fn apply_returns_skipped_when_systemctl_fails() {
        let executor = MockExecutor::fail();
        let resource = SystemdUnitResource::new("dunst.service".to_string(), &executor);
        assert!(
            matches!(resource.apply().unwrap(), ResourceChange::Skipped { .. }),
            "expected Skipped when systemctl enable fails"
        );
    }
}
