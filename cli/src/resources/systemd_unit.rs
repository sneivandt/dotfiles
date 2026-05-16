//! Systemd unit resource.
use std::sync::Arc;

use anyhow::Result;

use super::{Applicable, Resource, ResourceChange, ResourceState};
use crate::error::ResourceError;
use crate::exec::Executor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SystemdScope {
    User,
    System,
}

impl SystemdScope {
    fn parse(scope: &str) -> Result<Self> {
        match scope {
            "user" => Ok(Self::User),
            "system" => Ok(Self::System),
            _ => Err(
                ResourceError::not_supported(format!("unsupported systemd scope '{scope}'")).into(),
            ),
        }
    }
}

/// A systemd unit resource that can be checked and enabled.
#[derive(Debug)]
pub struct SystemdUnitResource {
    /// Unit name (e.g. "clean-home-tmp.timer").
    pub name: String,
    /// Systemd scope (`user` or `system`).
    pub scope: String,
    /// Executor for running systemctl commands.
    executor: Arc<dyn Executor>,
}

impl SystemdUnitResource {
    /// Create a new systemd unit resource.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        scope: impl Into<String>,
        executor: Arc<dyn Executor>,
    ) -> Self {
        Self {
            name: name.into(),
            scope: scope.into(),
            executor,
        }
    }

    /// Create from a config entry.
    #[must_use]
    pub fn from_entry(
        entry: &crate::config::systemd_units::SystemdUnit,
        executor: Arc<dyn Executor>,
    ) -> Self {
        Self::new(entry.name.clone(), entry.scope.clone(), executor)
    }

    fn parsed_scope(&self) -> Result<SystemdScope> {
        SystemdScope::parse(&self.scope)
    }

    fn check_args(&self, scope: SystemdScope) -> Vec<&str> {
        match scope {
            SystemdScope::User => vec!["--user", "is-enabled", &self.name],
            SystemdScope::System => vec!["is-enabled", &self.name],
        }
    }

    fn apply_invocation(&self, scope: SystemdScope) -> (&'static str, Vec<&str>) {
        match scope {
            SystemdScope::User => ("systemctl", vec!["--user", "enable", "--now", &self.name]),
            SystemdScope::System => ("sudo", vec!["systemctl", "enable", "--now", &self.name]),
        }
    }
}

impl Applicable for SystemdUnitResource {
    fn description(&self) -> String {
        if self.scope == "user" {
            self.name.clone()
        } else {
            format!("{} ({} scope)", self.name, self.scope)
        }
    }

    fn apply(&self) -> Result<ResourceChange> {
        let scope = self.parsed_scope()?;
        let (program, args) = self.apply_invocation(scope);
        let result = self.executor.run_unchecked(program, &args)?;
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
        let scope = match self.parsed_scope() {
            Ok(scope) => scope,
            Err(e) => {
                return Ok(ResourceState::Invalid {
                    reason: e.to_string(),
                });
            }
        };
        let args = self.check_args(scope);
        let result = self.executor.run_unchecked("systemctl", &args)?;
        if result.success {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Missing)
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
        let resource =
            SystemdUnitResource::new("clean-home-tmp.timer".to_string(), "user", executor);
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
        assert_eq!(resource.scope, "user");
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
        let resource = SystemdUnitResource::new("dunst.service".to_string(), "user", executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_missing_when_systemctl_fails() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .returning(|_, _| Ok(fail_result()));
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new("dunst.service".to_string(), "user", executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    #[test]
    fn current_state_uses_system_scope_without_user_flag() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .withf(|program, args| program == "systemctl" && args == ["is-enabled", "sshd.service"])
            .returning(|_, _| Ok(ok_result()));
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new("sshd.service".to_string(), "system", executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_invalid_for_unknown_scope() {
        let mock = MockExecutor::new();
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new("dunst.service".to_string(), "global", executor);
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Invalid { .. }
        ));
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
        let resource = SystemdUnitResource::new("dunst.service".to_string(), "user", executor);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[test]
    fn apply_returns_skipped_when_systemctl_fails() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .returning(|_, _| Ok(fail_result()));
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new("dunst.service".to_string(), "user", executor);
        assert!(
            matches!(resource.apply().unwrap(), ResourceChange::Skipped { .. }),
            "expected Skipped when systemctl enable fails"
        );
    }

    #[test]
    fn apply_uses_sudo_for_system_scope() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .withf(|program, args| {
                program == "sudo" && args == ["systemctl", "enable", "--now", "sshd.service"]
            })
            .returning(|_, _| Ok(ok_result()));
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new("sshd.service".to_string(), "system", executor);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }
}
