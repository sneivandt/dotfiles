//! Systemd unit resource.
use std::sync::Arc;

use anyhow::Result;

use crate::domains::system::config::systemd_units::UnitScope;
use crate::engine::resource::ResourceError;
use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::exec::Executor;

/// A systemd unit resource that can be checked and enabled.
#[derive(Debug)]
pub struct SystemdUnitResource {
    /// Unit name (e.g. "clean-home-tmp.timer").
    pub name: String,
    /// Systemd scope.
    pub scope: UnitScope,
    /// Executor for running systemctl commands.
    executor: Arc<dyn Executor>,
}

impl SystemdUnitResource {
    /// Create a new systemd unit resource.
    #[must_use]
    pub fn new(name: impl Into<String>, scope: UnitScope, executor: Arc<dyn Executor>) -> Self {
        Self {
            name: name.into(),
            scope,
            executor,
        }
    }

    /// Create from a config entry.
    #[must_use]
    pub fn from_entry(
        entry: &crate::domains::system::config::systemd_units::SystemdUnit,
        executor: Arc<dyn Executor>,
    ) -> Self {
        Self::new(entry.name.clone(), entry.scope.clone(), executor)
    }

    fn check_args(&self) -> ResourceResult<Vec<&str>> {
        match self.scope {
            UnitScope::User => Ok(vec!["--user", "is-enabled", &self.name]),
            UnitScope::System => Ok(vec!["is-enabled", &self.name]),
            UnitScope::Invalid(ref value) => Err(ResourceError::not_supported(format!(
                "unsupported systemd scope '{value}'"
            ))),
        }
    }

    fn apply_invocation(&self) -> ResourceResult<(&'static str, Vec<&str>)> {
        match self.scope {
            UnitScope::User => Ok(("systemctl", vec!["--user", "enable", "--now", &self.name])),
            UnitScope::System => Ok(("sudo", vec!["systemctl", "enable", "--now", &self.name])),
            UnitScope::Invalid(ref value) => Err(ResourceError::not_supported(format!(
                "unsupported systemd scope '{value}'"
            ))),
        }
    }

    fn state_from_is_enabled(&self, result: &crate::infra::exec::ExecResult) -> ResourceState {
        if result.success {
            return ResourceState::Correct;
        }

        let output = command_output(result);
        if output.lines().any(|line| line.trim() == "disabled") {
            return ResourceState::Missing;
        }

        ResourceState::Unknown {
            reason: format!(
                "systemctl is-enabled {} failed ({}): {}",
                self.name,
                exit_status(result),
                output_if_present(&output)
            ),
        }
    }
}

fn command_output(result: &crate::infra::exec::ExecResult) -> String {
    let stdout = result.stdout.trim();
    let stderr = result.stderr.trim();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (false, false) => format!("{stdout}; {stderr}"),
    }
}

fn exit_status(result: &crate::infra::exec::ExecResult) -> String {
    result.code.map_or_else(
        || "terminated by signal".to_string(),
        |code| format!("exit {code}"),
    )
}

const fn output_if_present(output: &str) -> &str {
    if output.is_empty() {
        "no output"
    } else {
        output
    }
}

impl Resource for SystemdUnitResource {
    fn description(&self) -> String {
        match self.scope {
            UnitScope::User => self.name.clone(),
            UnitScope::System => format!("{} (system scope)", self.name),
            UnitScope::Invalid(ref value) => {
                format!("{} (invalid '{value}' scope)", self.name)
            }
        }
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        let (program, args) = self.apply_invocation()?;
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

impl IntrinsicState for SystemdUnitResource {
    fn current_state(&self) -> Result<ResourceState> {
        let args = match self.check_args() {
            Ok(args) => args,
            Err(error) => {
                return Ok(ResourceState::Invalid {
                    reason: error.to_string(),
                });
            }
        };
        let result = self.executor.run_unchecked("systemctl", &args)?;
        Ok(self.state_from_is_enabled(&result))
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
    use crate::infra::exec::{ExecResult, MockExecutor};

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
        let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::SystemExecutor);
        let resource = SystemdUnitResource::new(
            "clean-home-tmp.timer".to_string(),
            UnitScope::User,
            executor,
        );
        assert_eq!(resource.description(), "clean-home-tmp.timer");
    }

    #[test]
    fn from_entry_copies_name() {
        let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::SystemExecutor);
        let entry = crate::domains::system::config::systemd_units::SystemdUnit {
            name: "dunst.service".to_string(),
            scope: UnitScope::User,
        };
        let resource = SystemdUnitResource::from_entry(&entry, executor);
        assert_eq!(resource.name, "dunst.service");
        assert_eq!(resource.scope, UnitScope::User);
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
        let resource = SystemdUnitResource::new("dunst.service", UnitScope::User, executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_missing_when_systemctl_reports_disabled() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked().once().returning(|_, _| {
            Ok(ExecResult {
                stdout: "disabled\n".to_string(),
                ..fail_result()
            })
        });
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new("dunst.service", UnitScope::User, executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    #[test]
    fn current_state_unknown_when_systemctl_failure_is_ambiguous() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked().once().returning(|_, _| {
            Ok(ExecResult {
                stderr: "Failed to connect to bus".to_string(),
                ..fail_result()
            })
        });
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new("dunst.service", UnitScope::User, executor);
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Unknown { .. }
        ));
    }

    #[test]
    fn current_state_uses_system_scope_without_user_flag() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .withf(|program, args| program == "systemctl" && args == ["is-enabled", "sshd.service"])
            .returning(|_, _| Ok(ok_result()));
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new("sshd.service", UnitScope::System, executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_invalid_for_unknown_scope() {
        let mock = MockExecutor::new();
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new(
            "dunst.service",
            UnitScope::Invalid("global".to_string()),
            executor,
        );
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
        let resource = SystemdUnitResource::new("dunst.service", UnitScope::User, executor);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[test]
    fn apply_returns_skipped_when_systemctl_fails() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .returning(|_, _| Ok(fail_result()));
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new("dunst.service", UnitScope::User, executor);
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
        let resource = SystemdUnitResource::new("sshd.service", UnitScope::System, executor);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }
}
