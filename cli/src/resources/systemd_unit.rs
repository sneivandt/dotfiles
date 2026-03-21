//! Systemd user unit resource.
use std::sync::Arc;

use anyhow::Result;

use super::{Applicable, Resource, ResourceChange, ResourceState};
use crate::exec::Executor;

/// A systemd user unit resource that can be checked and enabled.
#[derive(Debug)]
pub struct SystemdUnitResource {
    /// Unit name (e.g. "clean-home-tmp.timer").
    pub name: String,
    /// Executor for running systemctl commands.
    executor: Arc<dyn Executor>,
}

impl SystemdUnitResource {
    /// Create a new systemd unit resource.
    #[must_use]
    pub fn new(name: String, executor: Arc<dyn Executor>) -> Self {
        Self { name, executor }
    }

    /// Create from a config entry.
    #[must_use]
    pub fn from_entry(
        entry: &crate::config::systemd_units::SystemdUnit,
        executor: Arc<dyn Executor>,
    ) -> Self {
        Self::new(entry.name.clone(), executor)
    }
}

impl Applicable for SystemdUnitResource {
    fn description(&self) -> String {
        self.name.clone()
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

impl Resource for SystemdUnitResource {
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
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::exec::{ExecResult, MockExecutor};

    fn ok_result() -> ExecResult {
        ExecResult {
            stdout: String::new(),
            stderr: String::new(),
            success: true,
            code: Some(0),
        }
    }

    fn fail_result() -> ExecResult {
        ExecResult {
            stdout: String::new(),
            stderr: String::new(),
            success: false,
            code: Some(1),
        }
    }

    #[test]
    fn description_returns_unit_name() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = SystemdUnitResource::new("clean-home-tmp.timer".to_string(), executor);
        assert_eq!(resource.description(), "clean-home-tmp.timer");
    }

    #[test]
    fn from_entry_copies_name() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let entry = crate::config::systemd_units::SystemdUnit {
            name: "dunst.service".to_string(),
            scope: "user".to_string(),
        };
        let resource = SystemdUnitResource::from_entry(&entry, executor);
        assert_eq!(resource.name, "dunst.service");
    }

    // ------------------------------------------------------------------
    // current_state
    // ------------------------------------------------------------------

    #[test]
    fn current_state_correct_when_systemctl_reports_enabled() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked().once().returning(|_, _| {
            Ok(ExecResult {
                stdout: "enabled\n".to_string(),
                ..ok_result()
            })
        });
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new("dunst.service".to_string(), executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_missing_when_systemctl_fails() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .returning(|_, _| Ok(fail_result()));
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new("dunst.service".to_string(), executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    // ------------------------------------------------------------------
    // apply
    // ------------------------------------------------------------------

    #[test]
    fn apply_returns_applied_when_systemctl_succeeds() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .returning(|_, _| Ok(ok_result()));
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new("dunst.service".to_string(), executor);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[test]
    fn apply_returns_skipped_when_systemctl_fails() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .returning(|_, _| Ok(fail_result()));
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new("dunst.service".to_string(), executor);
        assert!(
            matches!(resource.apply().unwrap(), ResourceChange::Skipped { .. }),
            "expected Skipped when systemctl enable fails"
        );
    }
}
