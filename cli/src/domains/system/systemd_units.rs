//! Task: configure systemd units.

use std::sync::Arc;

use anyhow::{Context as _, Result};

use crate::domains::system::config::systemd_units::{SystemdUnit, UnitScope};
use crate::domains::system::resources::systemd_unit::SystemdUnitResource;
use crate::engine::{
    Context, IntrinsicState, ProcessOpts, Requirement, RequirementState, ResourceState, Task,
    TaskMeta, TaskResult, process_requirement, process_resources,
};
use crate::infra::exec::CommandSpec;
use crate::infra::logging::OutputExt as _;

crate::engine::config_task! {
    /// Converge configured systemd unit enablement states.
    pub struct ConfigureSystemd(Vec<SystemdUnit>);
}

impl Task for ConfigureSystemd {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("Systemd units").with_selector("systemd")
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform().supports_systemd() && !self.config.get().is_empty() && !ctx.is_ci()
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        system_unit_needs_change(ctx, self.config.get())
    }

    fn ordering_requirements(&self) -> Vec<Requirement<'_>> {
        vec![
            systemctl_requirement(),
            Requirement::new("systemd-units-current", |ctx, _| {
                if !systemd_available(ctx)? {
                    return Ok(RequirementState::unsatisfied("systemd unavailable"));
                }
                let user_manager = user_manager_available(ctx, self.config.get())?;
                // Non-current units may still need package-provided inputs.
                for entry in self.config.get() {
                    match SystemdUnitResource::from_entry(
                        entry,
                        ctx.executor_arc(),
                        ctx.home(),
                        user_manager,
                    )
                    .current_state()
                    .with_context(|| format!("checking systemd unit {}", entry.name))?
                    {
                        ResourceState::Correct => {}
                        ResourceState::Unknown { reason } => {
                            anyhow::bail!("{}: {reason}", entry.name)
                        }
                        state @ (ResourceState::Missing
                        | ResourceState::Incorrect { .. }
                        | ResourceState::Invalid { .. }) => {
                            return Ok(RequirementState::unsatisfied(format!(
                                "{} is {state}",
                                entry.name
                            )));
                        }
                    }
                }
                Ok(RequirementState::Satisfied(()))
            }),
        ]
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let units = self.config.get();
        if units.is_empty() {
            return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
        }
        process_requirement(ctx, &systemctl_requirement(), |()| {
            if !systemd_available(ctx)? {
                return Ok(TaskResult::NotApplicable("systemd unavailable".to_string()));
            }

            let user_manager_available = user_manager_available(ctx, units)?;
            let system_reload_required = system_unit_needs_change(ctx, units);
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
        })
    }
}

fn systemctl_requirement() -> Requirement<'static> {
    Requirement::command("systemctl", "systemctl", "systemctl unavailable")
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

fn systemd_available(ctx: &Context) -> Result<bool> {
    if ctx.platform().is_wsl() {
        let result = ctx.executor().execute(
            CommandSpec::new("systemctl")
                .arg("is-system-running")
                .unchecked(),
        )?;
        anyhow::ensure!(
            result.code.is_some(),
            "systemd availability probe terminated without an exit code"
        );
        Ok(result.success || result.stdout.trim() == "degraded")
    } else {
        Ok(true)
    }
}

fn user_manager_available(ctx: &Context, units: &[SystemdUnit]) -> Result<bool> {
    if !units.iter().any(|unit| unit.scope == UnitScope::User) {
        return Ok(false);
    }
    if crate::infra::provisioning::is_arch_chroot(ctx.env().as_ref()) {
        ctx.log().debug(
            "Arch chroot provisioning has no user session; enabling user units offline for the next login",
        );
        return Ok(false);
    }
    let result = ctx.executor().execute(
        CommandSpec::new("systemctl")
            .args(&["--user", "show-environment"])
            .unchecked(),
    )?;
    anyhow::ensure!(
        result.code.is_some(),
        "user systemd manager probe terminated without an exit code"
    );
    let available = result.success;
    if !available {
        ctx.log().debug(
            "user systemd manager unavailable; enabling user units offline for the next login",
        );
    }
    Ok(available)
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
