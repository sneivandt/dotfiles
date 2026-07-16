//! Task: configure systemd units.

use anyhow::{Context as _, Result};

use crate::domains::system::config::systemd_units::{SystemdUnit, UnitScope};
use crate::domains::system::resources::systemd_unit::SystemdUnitResource;
use crate::engine::{
    Context, Domain, ExecutionPolicy, PlatformCapability, ProcessOpts, Task, TaskPhase, TaskResult,
    process_resources,
};
use crate::infra::ConfigHandle;

/// Enable and start systemd units.
#[derive(Debug)]
pub struct ConfigureSystemd {
    config: ConfigHandle<Vec<SystemdUnit>>,
}

impl ConfigureSystemd {
    /// Create the task with a handle to the systemd unit configuration.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<SystemdUnit>>) -> Self {
        Self { config }
    }
}

impl Task for ConfigureSystemd {
    fn name(&self) -> &'static str {
        "Configure systemd units"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Provision
    }

    fn domain(&self) -> Domain {
        Domain::System
    }

    fn execution_policies(&self) -> &[ExecutionPolicy] {
        const POLICIES: &[ExecutionPolicy] = &[
            PlatformCapability::Systemd.policy(),
            ExecutionPolicy::RequiresElevation,
        ];
        POLICIES
    }

    fn should_run(&self, ctx: &Context) -> bool {
        let system = ctx.system();
        system.platform().supports_systemd()
            && !self.config.read().is_empty()
            && system.which("systemctl")
            && systemd_available(ctx)
            && !system.is_ci()
    }

    fn needs_elevation(&self, _ctx: &Context) -> bool {
        self.config
            .read()
            .iter()
            .any(|unit| unit.scope == UnitScope::System)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let units = self.config.read();
        if units.is_empty() {
            return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
        }

        reload_daemons(ctx, &units)?;

        let system = ctx.system();
        let resources = units
            .iter()
            .map(|entry| SystemdUnitResource::from_entry(entry, system.executor_arc()));
        process_resources(
            ctx,
            resources,
            &ProcessOpts::install_missing("enable").sequential(),
        )
    }
}

fn systemd_available(ctx: &Context) -> bool {
    let system = ctx.system();
    if system.platform().is_wsl() {
        system
            .executor()
            .run_unchecked("systemctl", &["is-system-running"])
            .is_ok_and(|result| result.success || result.stdout.trim() == "degraded")
    } else {
        true
    }
}

fn reload_daemons(ctx: &Context, units: &[SystemdUnit]) -> Result<()> {
    if ctx.dry_run() {
        return Ok(());
    }

    if units.iter().any(|unit| unit.scope == UnitScope::User) {
        ctx.log().debug("running systemctl --user daemon-reload");
        ctx.executor()
            .run("systemctl", &["--user", "daemon-reload"])
            .context("reloading user systemd daemon")?;
        ctx.log().debug("user daemon-reload succeeded");
    }

    if units.iter().any(|unit| unit.scope == UnitScope::System) {
        ctx.log().debug("running sudo systemctl daemon-reload");
        ctx.executor()
            .run("sudo", &["systemctl", "daemon-reload"])
            .context("reloading system systemd daemon")?;
        ctx.log().debug("system daemon-reload succeeded");
    }

    Ok(())
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
    use crate::domains::system::config::systemd_units::SystemdUnit;
    use crate::engine::{Context, Task, TaskResult};
    use crate::infra::ConfigHandle;
    use crate::infra::exec::{ExecResult, MockExecutor};
    use crate::infra::platform::{Os, Platform};
    use crate::test_helpers::{
        ContextBuilder, empty_config, make_context, make_linux_context,
        make_platform_context_with_which,
    };
    use std::path::PathBuf;
    use std::sync::Arc;

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

    fn disabled_result() -> ExecResult {
        ExecResult {
            stdout: "disabled\n".to_string(),
            ..fail_result()
        }
    }

    #[test]
    fn should_run_false_on_windows() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: UnitScope::User,
        });
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_platform_context_with_which(config, Os::Windows, false, true);
        assert!(!ConfigureSystemd::new(units).should_run(&ctx));
    }

    #[test]
    fn should_run_false_when_units_empty() {
        let config = empty_config(PathBuf::from("/tmp"));
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_platform_context_with_which(config, Os::Linux, false, true);
        assert!(!ConfigureSystemd::new(units).should_run(&ctx));
    }

    #[test]
    fn should_run_false_when_systemctl_not_found() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: UnitScope::User,
        });
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_linux_context(config); // which() returns false
        assert!(!ConfigureSystemd::new(units).should_run(&ctx));
    }

    #[test]
    fn should_run_false_when_ci() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: UnitScope::User,
        });
        let units = ConfigHandle::new(config.units.clone());
        let ctx = ContextBuilder::new(config)
            .os(Os::Linux)
            .which(true)
            .ci(true)
            .build();
        assert!(!ConfigureSystemd::new(units).should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_linux_with_units_and_systemctl() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: UnitScope::User,
        });
        let units = ConfigHandle::new(config.units.clone());
        let ctx = ContextBuilder::new(config)
            .os(Os::Linux)
            .which(true)
            .ci(false)
            .build();
        assert!(ConfigureSystemd::new(units).should_run(&ctx));
    }

    // ------------------------------------------------------------------
    // ConfigureSystemd::run
    // ------------------------------------------------------------------

    /// Build a context backed by `MockExecutor` for `run()` tests.
    fn make_systemd_context(config: crate::Config, executor: MockExecutor) -> Context {
        make_context(config, Platform::new(Os::Linux, false), Arc::new(executor))
    }

    #[test]
    fn run_calls_daemon_reload_before_enabling_unit() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: UnitScope::User,
        });
        // Ordered expectations:
        //   1. run("systemctl", ["--user", "daemon-reload"]) -> success
        //   2. run_unchecked("systemctl", ["--user", "is-enabled", "dunst.service"]) -> disabled (Missing)
        //   3. run_unchecked("systemctl", ["--user", "enable", "--now", "dunst.service"]) → success
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        mock.expect_run()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(ok_result()));
        mock.expect_run_unchecked()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(disabled_result()));
        mock.expect_run_unchecked()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(ok_result()));
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_systemd_context(config, mock);

        let result = ConfigureSystemd::new(units).run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::OkWithMessage(_)),
            "expected OkWithMessage after daemon-reload + enable, got {result:?}"
        );
    }

    #[test]
    fn run_skips_daemon_reload_in_dry_run() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: UnitScope::User,
        });
        // In dry-run mode daemon-reload is NOT called (guarded by `!ctx.dry_run`).
        // current_state() still runs to decide whether change would be needed.
        //   1. run_unchecked("systemctl", ["--user", "is-enabled", "dunst.service"]) -> disabled (Missing)
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .returning(|_, _| Ok(disabled_result()));
        let units = ConfigHandle::new(config.units.clone());
        let mut ctx = make_systemd_context(config, mock);
        ctx = ctx.with_dry_run(true);

        let result = ConfigureSystemd::new(units).run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::DryRun),
            "expected DryRun when unit is missing in dry-run mode, got {result:?}"
        );
    }

    #[test]
    fn run_propagates_user_daemon_reload_failure() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: UnitScope::User,
        });
        let mut mock = MockExecutor::new();
        mock.expect_run()
            .once()
            .withf(|program, args| program == "systemctl" && args == ["--user", "daemon-reload"])
            .returning(|_, _| Err(anyhow::anyhow!("reload failed")));
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_systemd_context(config, mock);

        let error = ConfigureSystemd::new(units)
            .run(&ctx)
            .expect_err("daemon-reload failure should abort the task");
        assert!(error.to_string().contains("reloading user systemd daemon"));
    }

    #[test]
    fn run_propagates_system_daemon_reload_failure() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "sshd.service".to_string(),
            scope: UnitScope::System,
        });
        let mut mock = MockExecutor::new();
        mock.expect_run()
            .once()
            .withf(|program, args| program == "sudo" && args == ["systemctl", "daemon-reload"])
            .returning(|_, _| Err(anyhow::anyhow!("reload failed")));
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_systemd_context(config, mock);

        let error = ConfigureSystemd::new(units)
            .run(&ctx)
            .expect_err("daemon-reload failure should abort the task");
        assert!(
            error
                .to_string()
                .contains("reloading system systemd daemon")
        );
    }

    #[test]
    fn needs_sudo_true_for_system_scope_unit() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "sshd.service".to_string(),
            scope: UnitScope::System,
        });
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_platform_context_with_which(config, Os::Linux, false, true);

        assert!(ConfigureSystemd::new(units).requires_elevation(&ctx));
    }

    #[test]
    fn run_reloads_and_enables_system_scope_units_with_sudo() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "sshd.service".to_string(),
            scope: UnitScope::System,
        });
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        mock.expect_run()
            .once()
            .in_sequence(&mut seq)
            .withf(|program, args| program == "sudo" && args == ["systemctl", "daemon-reload"])
            .returning(|_, _| Ok(ok_result()));
        mock.expect_run_unchecked()
            .once()
            .in_sequence(&mut seq)
            .withf(|program, args| program == "systemctl" && args == ["is-enabled", "sshd.service"])
            .returning(|_, _| Ok(disabled_result()));
        mock.expect_run_unchecked()
            .once()
            .in_sequence(&mut seq)
            .withf(|program, args| {
                program == "sudo" && args == ["systemctl", "enable", "--now", "sshd.service"]
            })
            .returning(|_, _| Ok(ok_result()));
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_systemd_context(config, mock);

        let result = ConfigureSystemd::new(units).run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::OkWithMessage(_)),
            "expected OkWithMessage after system-scope daemon-reload + enable, got {result:?}"
        );
    }
}
