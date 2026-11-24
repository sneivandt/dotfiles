//! Systemd unit resource.
mod install_targets;
mod offline;
#[cfg(test)]
use install_targets::{has_install_directives, install_targets};
use offline::symlink_references;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domains::system::config::systemd_units::{UnitScope, safe_unit_name};
use crate::engine::resource::ResourceError;
use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::exec::{CommandSpec, Executor};
use crate::infra::provisioning::{
    DEFERRED_EXTENSIONS_MARKER, DeferredExtensionsState, FIRST_LOGIN_UNIT,
    deferred_extensions_state,
};

const THUNAR_SETTINGS_UNIT: &str = "thunar-view-settings.service";
const THUNAR_SETTINGS_SCRIPT: &str = ".config/Thunar/managed-view-settings.sh";

/// Properties requested together during batched live unit discovery.
pub(crate) const BATCH_SHOW_PROPERTIES: &str = "Id,LoadState,ActiveState,Type,Result,ExecMainStartTimestampMonotonic,ConditionResult,NeedDaemonReload";

/// Batched live probe output for a single systemd unit.
#[derive(Debug, Clone)]
pub(crate) struct BatchUnitProbe {
    /// Authoritative on-disk enablement line from `systemctl is-enabled`.
    pub(crate) is_enabled: String,
    /// Unit properties stanza from `systemctl show`.
    pub(crate) properties: String,
}

/// A systemd unit resource that converges its configured enablement state.
#[derive(Debug)]
pub struct SystemdUnitResource {
    /// Unit name (e.g. "clean-home-tmp.timer").
    pub name: String,
    /// Systemd scope.
    pub scope: UnitScope,
    /// Whether the unit should be enabled and started.
    pub enabled: bool,
    /// Executor for running systemctl commands.
    executor: Arc<dyn Executor>,
    /// Home directory containing user unit files.
    home: Option<PathBuf>,
    /// System-wide user unit directories, in lookup order.
    system_user_unit_dirs: Vec<PathBuf>,
    /// Whether this unit's service manager is available in the target.
    manager_available: bool,
    start: bool,
    retired_source: Option<PathBuf>,
    thunar_settings_script: Option<PathBuf>,
}

impl SystemdUnitResource {
    /// Create a new systemd unit resource.
    #[must_use]
    pub fn new(name: impl Into<String>, scope: UnitScope, executor: Arc<dyn Executor>) -> Self {
        Self {
            name: name.into(),
            scope,
            enabled: true,
            executor,
            home: None,
            system_user_unit_dirs: [
                "/etc/systemd/user",
                "/run/systemd/user",
                "/usr/local/lib/systemd/user",
                "/usr/lib/systemd/user",
                "/lib/systemd/user",
            ]
            .into_iter()
            .map(PathBuf::from)
            .collect(),
            manager_available: true,
            start: true,
            retired_source: None,
            thunar_settings_script: None,
        }
    }

    /// Create from a config entry.
    #[must_use]
    pub fn from_entry(
        entry: &crate::domains::system::config::systemd_units::SystemdUnit,
        executor: Arc<dyn Executor>,
        home: &Path,
        manager_available: bool,
        graphical_session_active: bool,
    ) -> Self {
        let mut resource = Self::new(entry.name.clone(), entry.scope.clone(), executor);
        resource.enabled = entry.enabled;
        resource.home = Some(home.to_path_buf());
        resource.manager_available = manager_available;
        resource.start = !entry.graphical || graphical_session_active;
        if entry.enabled && entry.scope == UnitScope::User && entry.name == THUNAR_SETTINGS_UNIT {
            resource.thunar_settings_script = Some(home.join(THUNAR_SETTINGS_SCRIPT));
        }
        resource
    }

    /// Whether this resource can participate in a batched live `systemctl show` probe.
    #[must_use]
    pub(crate) fn is_batch_probeable(&self) -> bool {
        self.retired_source.is_none()
            && self.manager_available
            && !matches!(self.scope, UnitScope::Invalid(_))
            && safe_unit_name(&self.name)
    }

    /// Query multiple live units in one batched `is-enabled` + `show` pair for
    /// a scope.
    ///
    /// Uses `systemctl is-enabled` for authoritative on-disk enablement (rather
    /// than `systemctl show`'s cached `UnitFileState`) and `systemctl show` for
    /// runtime and reload properties. Returns an empty map when fewer than two
    /// units are supplied, when `is-enabled` writes stderr or lacks an exit
    /// code, when `show` fails, or when line/stanza counts do not match
    /// `names`, allowing callers to fall back to individual unit probes.
    pub(crate) fn batch_probe_scope(
        executor: &dyn Executor,
        scope: &UnitScope,
        names: &[&str],
    ) -> ResourceResult<std::collections::HashMap<String, BatchUnitProbe>> {
        if names.len() < 2 || matches!(scope, UnitScope::Invalid(_)) {
            return Ok(std::collections::HashMap::new());
        }

        let mut enabled_args = Vec::with_capacity(names.len().saturating_add(2));
        if *scope == UnitScope::User {
            enabled_args.push("--user");
        }
        enabled_args.push("is-enabled");
        enabled_args.extend_from_slice(names);

        // `systemctl is-enabled u1 u2 ...` exits non-zero when all units are
        // disabled/not-found, but still prints one authoritative state per unit.
        let enabled_result = executor.execute(
            CommandSpec::new("systemctl")
                .args(&enabled_args)
                .unchecked(),
        )?;
        if enabled_result.code.is_none() || !enabled_result.stderr.trim().is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let enabled_normalized = enabled_result.stdout.replace("\r\n", "\n");
        let enabled_trimmed = enabled_normalized.trim_end_matches('\n');
        if enabled_trimmed.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let enabled_states: Vec<&str> = enabled_trimmed.split('\n').map(str::trim).collect();
        if enabled_states.len() != names.len() {
            return Ok(std::collections::HashMap::new());
        }

        let property_arg = format!("--property={BATCH_SHOW_PROPERTIES}");
        let mut show_args = Vec::with_capacity(names.len().saturating_add(3));
        if *scope == UnitScope::User {
            show_args.push("--user");
        }
        show_args.push("show");
        show_args.push(&property_arg);
        show_args.extend_from_slice(names);

        let show_result =
            executor.execute(CommandSpec::new("systemctl").args(&show_args).unchecked())?;
        if !show_result.success {
            return Ok(std::collections::HashMap::new());
        }
        let show_normalized = show_result.stdout.replace("\r\n", "\n");
        let show_trimmed = show_normalized.trim_matches('\n');
        if show_trimmed.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let stanzas: Vec<&str> = show_trimmed.split("\n\n").collect();
        if stanzas.len() != names.len() {
            return Ok(std::collections::HashMap::new());
        }
        Ok(names
            .iter()
            .zip(enabled_states)
            .zip(stanzas)
            .map(|((name, is_enabled), stanza)| {
                (
                    (*name).to_owned(),
                    BatchUnitProbe {
                        is_enabled: is_enabled.to_owned(),
                        properties: stanza.to_owned(),
                    },
                )
            })
            .collect())
    }

    /// Derive this resource's state from a batched `is-enabled` + `show` probe.
    ///
    /// Returns `Ok(None)` for alias, runtime-only, indirect, or ambiguous states
    /// so callers fall back to individual unit discovery.
    pub(crate) fn state_from_batch_probe(
        &self,
        probe: &BatchUnitProbe,
    ) -> ResourceResult<Option<ResourceState>> {
        if !self.is_batch_probeable() {
            return Ok(None);
        }
        if systemd_property(&probe.properties, "Id") != Some(self.name.as_str()) {
            return Ok(None);
        }
        let enablement = match probe.is_enabled.as_str() {
            "enabled" => {
                if self.enabled {
                    ResourceState::Correct
                } else {
                    ResourceState::Incorrect {
                        current: "enabled".into(),
                    }
                }
            }
            "disabled" | "linked" | "linked-runtime" | "masked" => {
                if self.enabled {
                    ResourceState::Missing
                } else {
                    ResourceState::Correct
                }
            }
            "static" => {
                if self.enabled {
                    ResourceState::Invalid {
                        reason:
                            "static unit has no [Install] instructions for persistent enablement"
                                .into(),
                    }
                } else {
                    ResourceState::Correct
                }
            }
            "not-found" if !self.enabled => ResourceState::Correct,
            _ => return Ok(None),
        };

        if self.scope == UnitScope::User
            && self.home.is_some()
            && matches!(enablement, ResourceState::Incorrect { .. })
            && let Some(conflict) = self.global_enablement_conflict()?
        {
            return Ok(Some(conflict));
        }
        if matches!(enablement, ResourceState::Correct)
            && let Some(state) = self.live_user_link_state()?
            && !matches!(state, ResourceState::Correct)
        {
            return Ok(Some(state));
        }
        if !matches!(enablement, ResourceState::Correct)
            || (self.enabled && !self.start)
            || (!self.enabled && probe.is_enabled == "not-found")
        {
            return Ok(Some(enablement));
        }
        if self.scope == UnitScope::System
            && systemd_property(&probe.properties, "NeedDaemonReload") == Some("yes")
        {
            return Ok(Some(ResourceState::Incorrect {
                current: "systemd has a stale unit definition".into(),
            }));
        }
        if let Some(state) = self.marker_state_if_idle(&probe.properties)? {
            return Ok(Some(state));
        }
        let state = runtime_state(self.enabled, &probe.properties);
        if matches!(state, ResourceState::Correct)
            && let Some(script) = &self.thunar_settings_script
        {
            return self.thunar_settings_state(script).map(Some);
        }
        Ok(Some(state))
    }

    fn check_args<'a>(&'a self, args: &[&'a str]) -> ResourceResult<Vec<&'a str>> {
        match self.scope {
            UnitScope::User => Ok([&["--user"][..], args, &[&self.name]].concat()),
            UnitScope::System if !self.manager_available => {
                Ok([&["--root=/"][..], args, &[&self.name]].concat())
            }
            UnitScope::System => Ok([args, &[&self.name]].concat()),
            UnitScope::Invalid(ref value) => Err(ResourceError::not_supported(format!(
                "unsupported systemd scope '{value}'"
            ))),
        }
    }

    pub(crate) fn retired_nautilus_settings(
        root: &Path,
        home: &Path,
        executor: Arc<dyn Executor>,
    ) -> Self {
        let mut resource = Self::new("nautilus-settings.service", UnitScope::User, executor);
        resource.enabled = false;
        resource.home = Some(home.to_path_buf());
        resource.retired_source = Some(
            root.join("symlinks/config/systemd/user")
                .join(&resource.name),
        );
        resource
    }

    fn change_invocation<'a>(
        &'a self,
        args: &[&'a str],
    ) -> ResourceResult<(&'static str, Vec<&'a str>)> {
        match self.scope {
            UnitScope::User => Ok(("systemctl", [&["--user"][..], args, &[&self.name]].concat())),
            UnitScope::System if !self.manager_available => Ok((
                "sudo",
                [&["systemctl", "--root=/"][..], args, &[&self.name]].concat(),
            )),
            UnitScope::System => Ok(("sudo", [&["systemctl"][..], args, &[&self.name]].concat())),
            UnitScope::Invalid(ref value) => Err(ResourceError::not_supported(format!(
                "unsupported systemd scope '{value}'"
            ))),
        }
    }

    fn retired_links(&self, source: &Path) -> ResourceResult<Vec<PathBuf>> {
        let user_dir = self.offline_user_dir()?;
        let installed = user_dir.join(&self.name);
        let owned_unit = symlink_references(&installed, source);
        let wants = crate::infra::fs::checked_directory_chain(
            &user_dir,
            Path::new("graphical-session.target.wants"),
        )?;
        let enabled = wants.join(&self.name);
        let mut links = Vec::new();
        if symlink_references(&enabled, source)
            || (owned_unit && symlink_references(&enabled, &installed))
        {
            links.push(enabled);
        }
        if owned_unit {
            links.push(installed);
        }
        Ok(links)
    }

    fn change_live_unit(&self, args: &[&str]) -> ResourceResult<ResourceChange> {
        let (program, command_args) = self.change_invocation(args)?;
        let result = self
            .executor
            .execute(CommandSpec::new(program).args(&command_args).unchecked())?;
        if result.success {
            Ok(ResourceChange::Applied)
        } else {
            Ok(ResourceChange::unusable(format!(
                "{program} {} failed ({}); stdout: {}; stderr: {}",
                command_args.join(" "),
                exit_status(&result),
                output_if_present(result.stdout.trim()),
                output_if_present(result.stderr.trim())
            )))
        }
    }

    fn state_from_is_enabled(&self, result: &crate::infra::exec::ExecResult) -> ResourceState {
        let output = command_output(result);
        if !self.enabled && result.stdout.trim() == "masked-runtime" {
            return ResourceState::Unknown {
                reason: "a temporary mask hides persistent enablement; retry after the mask expires or is lifted"
                    .into(),
            };
        }
        if result.success && result.stdout.trim() == "enabled-runtime" {
            // Runtime enablement disappears at reboot; enable without --runtime
            // to establish the persistent links required by desired state.
            return if self.enabled {
                ResourceState::Missing
            } else if !self.manager_available {
                // No live target manager: the runtime links disappear on boot.
                ResourceState::Correct
            } else {
                ResourceState::Incorrect { current: output }
            };
        }
        if result.success {
            let state = result.stdout.trim();
            // A static unit has no installation instructions, but it can still be stopped.
            if self.enabled && state == "static" {
                return ResourceState::Invalid {
                    reason: "static unit has no [Install] instructions for persistent enablement"
                        .into(),
                };
            }
            if state == "indirect" {
                return ResourceState::Invalid {
                    reason: "indirect unit has ambiguous enablement; declare the unit with direct install links".into(),
                };
            }
            return if self.enabled || state == "static" {
                ResourceState::Correct
            } else {
                ResourceState::Incorrect {
                    current: output_if_present(&output).to_string(),
                }
            };
        }

        let disabled = output.lines().map(str::trim).any(|state| {
            matches!(
                state,
                "disabled" | "linked" | "linked-runtime" | "masked" | "masked-runtime"
            )
        });
        if disabled {
            return if self.enabled {
                ResourceState::Missing
            } else {
                ResourceState::Correct
            };
        }

        if !self.enabled && (output.contains("not-found") || output.contains("could not be found"))
        {
            return ResourceState::Correct;
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

    fn alias_state(&self) -> ResourceResult<ResourceState> {
        if !self.manager_available {
            return Ok(ResourceState::Invalid {
                reason: format!("{} is an alias. Declare the canonical unit name", self.name),
            });
        }
        let args = self.check_args(&["show", "--property=Id", "--value"])?;
        let result = self
            .executor
            .execute(CommandSpec::new("systemctl").args(&args).unchecked())?;
        let id = result.stdout.trim();
        let reason = if result.success && safe_unit_name(id) && id != self.name {
            format!("{} is an alias. Declare the canonical unit {id}", self.name)
        } else {
            format!("{} is an alias. Declare the canonical unit name", self.name)
        };
        Ok(ResourceState::Invalid { reason })
    }

    fn thunar_settings_state(&self, script: &Path) -> ResourceResult<ResourceState> {
        let result = self.executor.execute(
            CommandSpec::new("/bin/sh")
                .arg(script.as_os_str())
                .arg("check")
                .unchecked(),
        )?;
        Ok(match result.code {
            Some(0) => ResourceState::Correct,
            Some(1) => ResourceState::Incorrect {
                current: "Thunar settings differ from the managed defaults".into(),
            },
            _ => ResourceState::Unknown {
                reason: format!(
                    "reading Thunar settings failed ({}): {}",
                    exit_status(&result),
                    output_if_present(&command_output(&result))
                ),
            },
        })
    }

    fn marker_state_if_idle(&self, properties: &str) -> ResourceResult<Option<ResourceState>> {
        // Start history can be stale or lost when the user manager restarts.
        if self.enabled
            && self.scope == UnitScope::User
            && self.name == FIRST_LOGIN_UNIT
            && systemd_property(properties, "ActiveState") == Some("inactive")
            && let Some(home) = &self.home
        {
            let marker = home.join(DEFERRED_EXTENSIONS_MARKER);
            return Ok(Some(match deferred_extensions_state(&marker)? {
                DeferredExtensionsState::Absent => ResourceState::Correct,
                DeferredExtensionsState::Pending => ResourceState::Incorrect {
                    current: "VS Code extension provisioning is pending".into(),
                },
                DeferredExtensionsState::Invalid => ResourceState::Invalid {
                    reason: format!("{} is not a regular file", marker.display()),
                },
            }));
        }
        Ok(None)
    }
}

fn systemd_property<'a>(properties: &'a str, key: &str) -> Option<&'a str> {
    properties
        .lines()
        .filter_map(|line| line.split_once('='))
        .find_map(|(name, value)| (name == key).then_some(value))
}

fn runtime_state(enabled: bool, properties: &str) -> ResourceState {
    let property = |key| systemd_property(properties, key);
    let active = property("ActiveState").unwrap_or("");
    if enabled && active == "inactive" && property("ConditionResult") == Some("no") {
        return ResourceState::Incorrect {
            current: "previous start was skipped by a condition".into(),
        };
    }
    let successful_oneshot = active == "inactive"
        && property("Type") == Some("oneshot")
        && property("Result") == Some("success")
        && property("ExecMainStartTimestampMonotonic")
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|value| value > 0);
    let matches = match active {
        "active" | "reloading" | "refreshing" | "activating" => enabled,
        "inactive" | "failed" => !enabled || successful_oneshot,
        "deactivating" => false,
        _ => {
            return ResourceState::Unknown {
                reason: format!("unrecognized systemd ActiveState: {active:?}"),
            };
        }
    };
    if matches {
        ResourceState::Correct
    } else {
        ResourceState::Incorrect {
            current: format!("runtime state is {active}"),
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
        if self.retired_source.is_some() {
            return format!("obsolete links for {}", self.name);
        }
        let desired = if self.enabled { "" } else { " disabled" };
        match self.scope {
            UnitScope::User => format!("{}{desired}", self.name),
            UnitScope::System => format!("{}{desired} (system scope)", self.name),
            UnitScope::Invalid(ref value) => {
                format!("{} (invalid '{value}' scope)", self.name)
            }
        }
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        if !safe_unit_name(&self.name) {
            return Err(ResourceError::not_supported(
                "unit name must be a single safe systemd name, not a path or option",
            ));
        }
        if let Some(source) = &self.retired_source {
            let links = self.retired_links(source)?;
            if links.is_empty() {
                return Ok(ResourceChange::AlreadyCorrect);
            }
            for link in links {
                if !self.retired_links(source)?.contains(&link) {
                    return Err(
                        anyhow::anyhow!("retired unit link changed during removal; retry").into(),
                    );
                }
                std::fs::remove_file(link)?;
            }
            return Ok(ResourceChange::Applied);
        }
        if !self.manager_available {
            match self.scope {
                UnitScope::User if self.enabled => return self.enable_offline(),
                UnitScope::User => return self.disable_offline(),
                UnitScope::System => {
                    let action = if self.enabled { "enable" } else { "disable" };
                    return self.change_live_unit(&[action]);
                }
                UnitScope::Invalid(_) => {}
            }
        }
        if self.enabled {
            let link_state = self.live_user_link_state()?;
            if let Some(ResourceState::Invalid { reason }) = link_state {
                return Ok(ResourceChange::unusable(reason));
            }
            if matches!(
                link_state,
                Some(ResourceState::Incorrect { .. } | ResourceState::Missing)
            ) {
                let links = self.offline_enablement_links()?;
                self.reconcile_user_links(&links, true)?;
                self.executor
                    .execute(CommandSpec::new("systemctl").args(&["--user", "daemon-reload"]))?;
                if !self.start {
                    return Ok(ResourceChange::Applied);
                }
                if self.name == FIRST_LOGIN_UNIT {
                    return self.change_live_unit(&["start", "--no-block"]);
                }
                let action = if self.thunar_settings_script.is_some() {
                    "restart"
                } else {
                    "start"
                };
                return self.change_live_unit(&[action]);
            }
            if !self.start {
                return self.change_live_unit(&["enable"]);
            }
            if self.scope == UnitScope::User && self.name == FIRST_LOGIN_UNIT {
                // The service runs Dotfiles and waits for the current update's locks.
                return self.change_live_unit(&["enable", "--now", "--no-block"]);
            }
            if self.thunar_settings_script.is_some() {
                let enablement = self.change_live_unit(&["enable"])?;
                if !matches!(enablement, ResourceChange::Applied) {
                    return Ok(enablement);
                }
                return self.change_live_unit(&["restart"]);
            }
            return self.change_live_unit(&["enable", "--now"]);
        }
        // Per-user disable succeeds with just a warning when a global link
        // will activate the same unit again at the next login.
        if self.scope == UnitScope::User
            && self.home.is_some()
            && let Some(ResourceState::Invalid { reason }) = self.global_enablement_conflict()?
        {
            return Ok(ResourceChange::unusable(reason));
        }
        // systemctl scopes disable to one enablement directory at a time.
        let runtime = self.change_live_unit(&["disable", "--runtime"])?;
        if !matches!(runtime, ResourceChange::Applied) {
            return Ok(runtime);
        }
        self.change_live_unit(&["disable", "--now"])
    }
}

impl IntrinsicState for SystemdUnitResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        if let Some(source) = &self.retired_source {
            return Ok(if self.retired_links(source)?.is_empty() {
                ResourceState::Correct
            } else {
                ResourceState::Incorrect {
                    current: "obsolete managed unit links remain".into(),
                }
            });
        }
        if !safe_unit_name(&self.name) {
            return Ok(ResourceState::Invalid {
                reason: "unit name must be a single safe systemd name, not a path or option".into(),
            });
        }
        if self.scope == UnitScope::User && !self.manager_available {
            return self.offline_current_state();
        }
        let args = match self.check_args(&["is-enabled"]) {
            Ok(args) => args,
            Err(error) => {
                return Ok(ResourceState::Invalid {
                    reason: error.to_string(),
                });
            }
        };
        let result = self
            .executor
            .execute(CommandSpec::new("systemctl").args(&args).unchecked())?;
        if result.success && result.stdout.trim() == "alias" {
            return self.alias_state();
        }
        let enablement = self.state_from_is_enabled(&result);
        if !self.enabled
            && self.scope == UnitScope::User
            && self.home.is_some()
            && matches!(enablement, ResourceState::Incorrect { .. })
            && let Some(conflict) = self.global_enablement_conflict()?
        {
            return Ok(conflict);
        }
        if matches!(enablement, ResourceState::Correct)
            && let Some(state) = self.live_user_link_state()?
            && !matches!(state, ResourceState::Correct)
        {
            return Ok(state);
        }
        // Missing disabled units need no runtime probe. Ambiguous enablement
        // must keep its diagnostic instead of being treated as converged.
        if !self.manager_available
            || !matches!(enablement, ResourceState::Correct)
            || (self.enabled && !self.start)
            || (!self.enabled
                && (command_output(&result).contains("not-found")
                    || command_output(&result).contains("could not be found")))
        {
            return Ok(enablement);
        }
        let runtime_args = self.check_args(&[
            "show",
            "--property=ActiveState,Type,Result,ExecMainStartTimestampMonotonic,ConditionResult,NeedDaemonReload",
        ])?;
        let runtime_result = self.executor.execute(
            CommandSpec::new("systemctl")
                .args(&runtime_args)
                .unchecked(),
        )?;
        if !runtime_result.success {
            return Ok(ResourceState::Unknown {
                reason: format!(
                    "systemctl show {} failed ({}): {}",
                    self.name,
                    exit_status(&runtime_result),
                    output_if_present(&command_output(&runtime_result))
                ),
            });
        }
        let properties = &runtime_result.stdout;
        if self.scope == UnitScope::System
            && systemd_property(properties, "NeedDaemonReload") == Some("yes")
        {
            return Ok(ResourceState::Incorrect {
                current: "systemd has a stale unit definition".into(),
            });
        }
        if let Some(state) = self.marker_state_if_idle(properties)? {
            return Ok(state);
        }
        let state = runtime_state(self.enabled, properties);
        if matches!(state, ResourceState::Correct)
            && let Some(script) = &self.thunar_settings_script
        {
            return self.thunar_settings_state(script);
        }
        Ok(state)
    }
}

#[cfg(test)]
#[path = "tests/systemd_unit.rs"]
mod tests;
