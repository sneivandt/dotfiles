//! Task: configure systemd units.

use std::sync::Arc;

use anyhow::{Context as _, Result};

use crate::domains::system::config::systemd_units::{SystemdUnit, UnitScope};
use crate::domains::system::resources::systemd_unit::SystemdUnitResource;
use crate::engine::{
    Context, IntrinsicState, ProcessOpts, Requirement, RequirementState, ResourceState, Task,
    TaskMeta, TaskResult, process_requirement, process_resources,
};
use crate::infra::exec::{CommandSpec, OutputLog};
use crate::infra::logging::OutputExt as _;

crate::engine::config_task! {
    /// Converge configured systemd unit enablement states.
    pub struct ConfigureSystemd(Vec<SystemdUnit>);
}

impl ConfigureSystemd {
    pub(crate) const fn is_applicable(
        platform: crate::infra::platform::Platform,
        has_declarations: bool,
        is_ci: bool,
    ) -> bool {
        platform.supports_systemd() && has_declarations && !is_ci
    }
}

impl Task for ConfigureSystemd {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("Systemd units").with_selector("systemd")
    }

    fn should_run(&self, ctx: &Context) -> bool {
        Self::is_applicable(ctx.platform(), !self.config.get().is_empty(), ctx.is_ci())
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
                let graphical_session =
                    graphical_session_active(ctx, self.config.get(), user_manager)?;
                // Non-current units may still need package-provided inputs.
                for entry in self.config.get() {
                    match SystemdUnitResource::from_entry(
                        entry,
                        ctx.executor_arc(),
                        ctx.home(),
                        target_manager_available(ctx, &entry.scope, user_manager),
                        graphical_session,
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
        anyhow::ensure!(
            super::config::systemd_units::validate_conflicts(units).is_empty(),
            "conflicting systemd enablement declarations; keep one desired state per scope and unit"
        );
        anyhow::ensure!(
            units
                .iter()
                .all(|unit| !unit.graphical || unit.scope == UnitScope::User),
            "graphical units require scope = 'user'"
        );
        if units.is_empty() {
            return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
        }
        process_requirement(ctx, &systemctl_requirement(), |()| {
            if !systemd_available(ctx)? {
                return Ok(TaskResult::NotApplicable("systemd unavailable".to_string()));
            }

            let user_manager_available = user_manager_available(ctx, units)?;
            let graphical_session = graphical_session_active(ctx, units, user_manager_available)?;
            let system_reload_required =
                !crate::infra::provisioning::is_arch_chroot(ctx.env().as_ref())
                    && system_unit_needs_change(ctx, units);
            reload_daemons(ctx, user_manager_available, system_reload_required)?;

            let executor = ctx.executor_arc();
            let retired = should_retire_nautilus_settings(units).then(|| {
                SystemdUnitResource::retired_nautilus_settings(
                    ctx.root(),
                    ctx.home(),
                    Arc::clone(&executor),
                )
            });
            let resources = units.iter().map(|entry| {
                SystemdUnitResource::from_entry(
                    entry,
                    Arc::clone(&executor),
                    ctx.home(),
                    target_manager_available(ctx, &entry.scope, user_manager_available),
                    graphical_session,
                )
            });
            process_resources(
                ctx,
                retired.into_iter().chain(resources),
                &ProcessOpts::lenient("configure").sequential(),
            )
        })
    }
}

fn should_retire_nautilus_settings(units: &[SystemdUnit]) -> bool {
    units.iter().any(|entry| {
        entry.scope == UnitScope::User && entry.name == "gtk-settings.service" && entry.enabled
    }) && !units
        .iter()
        .any(|entry| entry.scope == UnitScope::User && entry.name == "nautilus-settings.service")
}

fn systemctl_requirement() -> Requirement<'static> {
    Requirement::command("systemctl", "systemctl", "systemctl unavailable")
}

fn target_manager_available(ctx: &Context, scope: &UnitScope, user_manager: bool) -> bool {
    match scope {
        UnitScope::System => !crate::infra::provisioning::is_arch_chroot(ctx.env().as_ref()),
        UnitScope::User => user_manager,
        UnitScope::Invalid(_) => false,
    }
}

fn system_unit_needs_change(ctx: &Context, units: &[SystemdUnit]) -> bool {
    let executor = ctx.executor_arc();
    units
        .iter()
        .filter(|unit| unit.scope == UnitScope::System)
        .any(|unit| {
            matches!(
                SystemdUnitResource::from_entry(
                    unit,
                    Arc::clone(&executor),
                    ctx.home(),
                    target_manager_available(ctx, &unit.scope, false),
                    false
                )
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
        let code = result
            .code
            .context("systemd availability probe terminated without an exit code")?;
        match (code, result.stdout.trim()) {
            (0, "running") | (1.., "degraded") => Ok(true),
            (1.., "offline" | "initializing" | "starting" | "maintenance" | "stopping") => {
                Ok(false)
            }
            (0, state) => anyhow::bail!("unexpected systemctl is-system-running state: {state:?}"),
            // Failed bus or property queries may print "unknown" or no state.
            _ => Err(crate::infra::exec::ExecError::non_zero(
                "systemctl is-system-running",
                result,
            ))
            .context("checking WSL systemd state"),
        }
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
            .env("LC_ALL", "C")
            .output_log(OutputLog::Omit)
            .unchecked(),
    )?;
    anyhow::ensure!(
        result.code.is_some(),
        "user systemd manager probe terminated without an exit code"
    );
    if result.success {
        return Ok(true);
    }
    let error = result.stderr.trim();
    // Offline links cannot stop or start a running service. Only fall back
    // when the local bus is absent, not when it rejects or times out a query.
    let no_bus = error.split_once(": ").is_some_and(|(operation, reason)| {
        operation.starts_with("Failed to connect to ")
            && (matches!(reason, "No medium found" | "No such file or directory")
                || reason.starts_with("$DBUS_SESSION_BUS_ADDRESS and $XDG_RUNTIME_DIR not defined"))
    });
    anyhow::ensure!(no_bus, "cannot inspect the user systemd manager: {error}");
    ctx.log()
        .debug("user systemd manager unavailable; enabling user units offline for the next login");
    Ok(false)
}

fn graphical_session_active(
    ctx: &Context,
    units: &[SystemdUnit],
    user_manager_available: bool,
) -> Result<bool> {
    if !user_manager_available || !units.iter().any(|unit| unit.enabled && unit.graphical) {
        return Ok(false);
    }
    let result = ctx
        .executor()
        .execute(CommandSpec::new("systemctl").args(&[
            "--user",
            "show",
            "--property=ActiveState",
            "--value",
            "graphical-session.target",
        ]))
        .context("checking graphical session state")?;
    match result.stdout.trim() {
        "active" => Ok(true),
        "inactive" | "activating" | "deactivating" | "failed" => Ok(false),
        state => anyhow::bail!("unexpected graphical-session.target ActiveState: {state:?}"),
    }
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
