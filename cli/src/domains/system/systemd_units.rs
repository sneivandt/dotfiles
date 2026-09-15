//! Task: configure systemd units.

use std::sync::Arc;

use anyhow::{Context as _, Result};

use crate::domains::system::config::systemd_units::{SystemdUnit, UnitScope};
use crate::domains::system::resources::systemd_unit::SystemdUnitResource;
use crate::engine::{
    Context, IntrinsicState, ProcessOpts, ResourceState, Task, TaskMeta, TaskResult,
    process_resources,
};
use crate::infra::ConfigHandle;
use crate::infra::exec::CommandSpec;
use crate::infra::logging::OutputExt as _;

/// Converge configured systemd unit enablement states.
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
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("Systemd units").with_selector("systemd")
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform().supports_systemd()
            && !self.config.read().is_empty()
            && ctx.which("systemctl")
            && systemd_available(ctx)
            && !ctx.is_ci()
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        system_unit_needs_change(ctx, &self.config.read())
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let units = self.config.read().to_vec();
        if units.is_empty() {
            return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
        }

        let user_manager_available = user_manager_available(ctx, &units);
        let system_reload_required = system_unit_needs_change(ctx, &units);
        reload_daemons(ctx, user_manager_available, system_reload_required)?;

        let executor = ctx.executor_arc();
        let resources = units.iter().map(|entry| {
            SystemdUnitResource::from_entry(
                entry,
                Arc::clone(&executor),
                ctx.home(),
                user_manager_available,
            )
        });
        process_resources(
            ctx,
            resources,
            &ProcessOpts::lenient("configure").sequential(),
        )
    }
}

fn system_unit_needs_change(ctx: &Context, units: &[SystemdUnit]) -> bool {
    let executor = ctx.executor_arc();
    units
        .iter()
        .filter(|unit| unit.scope == UnitScope::System)
        .any(|unit| {
            matches!(
                SystemdUnitResource::from_entry(unit, Arc::clone(&executor), ctx.home(), true)
                    .current_state(),
                Ok(ResourceState::Missing | ResourceState::Incorrect { .. })
            )
        })
}

fn systemd_available(ctx: &Context) -> bool {
    if ctx.platform().is_wsl() {
        ctx.executor()
            .execute(
                CommandSpec::new("systemctl")
                    .arg("is-system-running")
                    .unchecked(),
            )
            .is_ok_and(|result| result.success || result.stdout.trim() == "degraded")
    } else {
        true
    }
}

fn user_manager_available(ctx: &Context, units: &[SystemdUnit]) -> bool {
    if !units.iter().any(|unit| unit.scope == UnitScope::User) {
        return false;
    }
    if crate::infra::provisioning::is_arch_chroot(ctx.env().as_ref()) {
        ctx.log().debug(
            "Arch chroot provisioning has no user session; enabling user units offline for the next login",
        );
        return false;
    }
    let available = ctx
        .executor()
        .execute(
            CommandSpec::new("systemctl")
                .args(&["--user", "show-environment"])
                .unchecked(),
        )
        .is_ok_and(|result| result.success);
    if !available {
        ctx.log().debug(
            "user systemd manager unavailable; enabling user units offline for the next login",
        );
    }
    available
}

fn reload_daemons(
    ctx: &Context,
    user_manager_available: bool,
    system_reload_required: bool,
) -> Result<()> {
    if ctx.dry_run() {
        return Ok(());
    }

    if user_manager_available {
        ctx.log().debug("running systemctl --user daemon-reload");
        ctx.executor()
            .execute(CommandSpec::new("systemctl").args(&["--user", "daemon-reload"]))
            .context("reloading user systemd daemon")?;
        ctx.log().debug("user daemon-reload succeeded");
    }

    if system_reload_required {
        ctx.log().debug("running sudo systemctl daemon-reload");
        ctx.executor()
            .execute(CommandSpec::new("sudo").args(&["systemctl", "daemon-reload"]))
            .context("reloading system systemd daemon")?;
        ctx.log().debug("system daemon-reload succeeded");
    }

    Ok(())
}

#[cfg(test)]
#[path = "tests/systemd_units.rs"]
mod tests;
