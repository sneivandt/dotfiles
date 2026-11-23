//! Task: configure systemd units.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result};

use crate::domains::system::config::systemd_units::{SystemdUnit, UnitScope};
use crate::domains::system::resources::systemd_unit::{BatchUnitProbe, SystemdUnitResource};
use crate::engine::{
    Context, IntrinsicState, ProcessOpts, Requirement, RequirementState, ResourceResult,
    ResourceState, Task, TaskMeta, TaskResult, process_requirement, process_resources_with_cache,
};
use crate::infra::exec::{CommandSpec, Executor, OutputLog};
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
                let executor = ctx.executor_arc();
                let resources: Vec<_> = self
                    .config
                    .get()
                    .iter()
                    .map(|entry| {
                        SystemdUnitResource::from_entry(
                            entry,
                            Arc::clone(&executor),
                            ctx.home(),
                            target_manager_available(ctx, &entry.scope, user_manager),
                            graphical_session,
                        )
                    })
                    .collect();
                let system_batch =
                    batch_probe_scope_resources(executor.as_ref(), &UnitScope::System, &resources)?;
                let user_batch =
                    batch_probe_scope_resources(executor.as_ref(), &UnitScope::User, &resources)?;
                // Non-current units may still need package-provided inputs.
                for resource in &resources {
                    let batch = match resource.scope {
                        UnitScope::System => Some(&system_batch),
                        UnitScope::User => Some(&user_batch),
                        UnitScope::Invalid(_) => None,
                    };
                    match resource_state_with_batch(resource, batch)
                        .with_context(|| format!("checking systemd unit {}", resource.name))?
                    {
                        ResourceState::Correct => {}
                        ResourceState::Unknown { reason } => {
                            anyhow::bail!("{}: {reason}", resource.name)
                        }
                        state @ (ResourceState::Missing
                        | ResourceState::Incorrect { .. }
                        | ResourceState::Invalid { .. }) => {
                            return Ok(RequirementState::unsatisfied(format!(
                                "{} is {state}",
                                resource.name
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
            let executor = ctx.executor_arc();
            let retired = should_retire_nautilus_settings(units).then(|| {
                SystemdUnitResource::retired_nautilus_settings(
                    ctx.root(),
                    ctx.home(),
                    Arc::clone(&executor),
                )
            });
            let resources: Vec<_> = units
                .iter()
                .map(|entry| {
                    SystemdUnitResource::from_entry(
                        entry,
                        Arc::clone(&executor),
                        ctx.home(),
                        target_manager_available(ctx, &entry.scope, user_manager_available),
                        graphical_session,
                    )
                })
                .collect();
            let inspect_system_units =
                !ctx.dry_run() && !crate::infra::provisioning::is_arch_chroot(ctx.env().as_ref());
            let (system_units_current, system_reload_required) =
                probe_system_reload_state(executor.as_ref(), &resources, inspect_system_units)?;
            reload_daemons(ctx, user_manager_available, system_reload_required)?;
            // Reinspect the whole system scope after a reload or when any unit
            // was unconfirmed. Changes to one unit can affect another.
            let batch_cache = Mutex::new(ScopeBatchCache::prepare(
                executor.as_ref(),
                &resources,
                system_units_current,
            )?);
            process_resources_with_cache(
                ctx,
                retired.into_iter().chain(resources),
                &batch_cache,
                |resource, cache| resolve_cached_unit_state(ctx, resource, cache),
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

struct ScopeBatchCache {
    system_units_current: bool,
    system: HashMap<String, BatchUnitProbe>,
    user: HashMap<String, BatchUnitProbe>,
}

impl ScopeBatchCache {
    fn prepare(
        executor: &dyn Executor,
        resources: &[SystemdUnitResource],
        system_units_current: bool,
    ) -> ResourceResult<Self> {
        let system = if system_units_current {
            HashMap::new()
        } else {
            batch_probe_scope_resources(executor, &UnitScope::System, resources)?
        };
        let user = batch_probe_scope_resources(executor, &UnitScope::User, resources)?;
        Ok(Self {
            system_units_current,
            system,
            user,
        })
    }

    fn get(&self, scope: &UnitScope, name: &str) -> Option<BatchUnitProbe> {
        match scope {
            UnitScope::System => self.system.get(name).cloned(),
            UnitScope::User => self.user.get(name).cloned(),
            UnitScope::Invalid(_) => None,
        }
    }

    fn invalidate_scope(&mut self, scope: &UnitScope) {
        match scope {
            UnitScope::System => self.system.clear(),
            UnitScope::User => self.user.clear(),
            UnitScope::Invalid(_) => {}
        }
    }
}

fn probe_system_reload_state(
    executor: &dyn Executor,
    resources: &[SystemdUnitResource],
    inspect_system_units: bool,
) -> ResourceResult<(bool, bool)> {
    if !inspect_system_units {
        return Ok((false, false));
    }
    let mut system_units_current = true;
    let pre_reload_system_batch =
        batch_probe_scope_resources(executor, &UnitScope::System, resources)?;
    for resource in resources
        .iter()
        .filter(|resource| resource.scope == UnitScope::System)
    {
        match resource_state_with_batch(resource, Some(&pre_reload_system_batch)) {
            Ok(ResourceState::Correct) => {}
            Ok(ResourceState::Missing | ResourceState::Incorrect { .. }) => {
                return Ok((false, true));
            }
            Err(error) if error.is_cancelled() => return Err(error),
            Ok(ResourceState::Invalid { .. } | ResourceState::Unknown { .. }) | Err(_) => {
                system_units_current = false;
            }
        }
    }
    Ok((system_units_current, false))
}

fn resolve_cached_unit_state(
    ctx: &Context,
    resource: &SystemdUnitResource,
    cache: &Mutex<ScopeBatchCache>,
) -> ResourceResult<ResourceState> {
    let cached = {
        let guard = cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if resource.scope == UnitScope::System && guard.system_units_current {
            return Ok(ResourceState::Correct);
        }
        guard.get(&resource.scope, &resource.name)
    };
    let state = match cached {
        Some(probe) => match resource.state_from_batch_probe(&probe)? {
            Some(state) => state,
            None => resource.current_state()?,
        },
        None => resource.current_state()?,
    };
    if !ctx.dry_run()
        && matches!(
            state,
            ResourceState::Missing | ResourceState::Incorrect { .. }
        )
    {
        cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .invalidate_scope(&resource.scope);
    }
    Ok(state)
}

fn batch_probe_scope_resources(
    executor: &dyn Executor,
    scope: &UnitScope,
    resources: &[SystemdUnitResource],
) -> ResourceResult<HashMap<String, BatchUnitProbe>> {
    let names: Vec<&str> = resources
        .iter()
        .filter(|resource| resource.scope == *scope && resource.is_batch_probeable())
        .map(|resource| resource.name.as_str())
        .collect();
    // Propagate typed cancellation; fall back to per-unit discovery on other
    // batch errors so one scope's failure does not abort unrelated units.
    match SystemdUnitResource::batch_probe_scope(executor, scope, &names) {
        Ok(batch) => Ok(batch),
        Err(error) if error.is_cancelled() => Err(error),
        Err(_) => Ok(HashMap::new()),
    }
}

fn resource_state_with_batch(
    resource: &SystemdUnitResource,
    batch: Option<&HashMap<String, BatchUnitProbe>>,
) -> ResourceResult<ResourceState> {
    if let Some(probe) = batch.and_then(|batch| batch.get(&resource.name))
        && let Some(state) = resource.state_from_batch_probe(probe)?
    {
        return Ok(state);
    }
    resource.current_state()
}

fn system_unit_needs_change(ctx: &Context, units: &[SystemdUnit]) -> bool {
    let executor = ctx.executor_arc();
    let resources: Vec<_> = units
        .iter()
        .filter(|unit| unit.scope == UnitScope::System)
        .map(|unit| {
            SystemdUnitResource::from_entry(
                unit,
                Arc::clone(&executor),
                ctx.home(),
                target_manager_available(ctx, &unit.scope, false),
                false,
            )
        })
        .collect();
    let Ok(batch) = batch_probe_scope_resources(executor.as_ref(), &UnitScope::System, &resources)
    else {
        return false;
    };
    resources.iter().any(|resource| {
        matches!(
            resource_state_with_batch(resource, Some(&batch)),
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
