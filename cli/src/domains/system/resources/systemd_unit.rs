//! Systemd unit resource.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domains::system::config::systemd_units::UnitScope;
use crate::engine::resource::ResourceError;
use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::exec::{CommandSpec, Executor};

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
    /// Whether the live user service manager is reachable.
    user_manager_available: bool,
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
            user_manager_available: true,
        }
    }

    /// Create from a config entry.
    #[must_use]
    pub fn from_entry(
        entry: &crate::domains::system::config::systemd_units::SystemdUnit,
        executor: Arc<dyn Executor>,
        home: &Path,
        user_manager_available: bool,
    ) -> Self {
        let mut resource = Self::new(entry.name.clone(), entry.scope.clone(), executor);
        resource.enabled = entry.enabled;
        resource.home = Some(home.to_path_buf());
        resource.user_manager_available = user_manager_available;
        resource
    }

    fn check_args<'a>(&'a self, args: &[&'a str]) -> ResourceResult<Vec<&'a str>> {
        match self.scope {
            UnitScope::User => Ok([&["--user"][..], args, &[&self.name]].concat()),
            UnitScope::System => Ok([args, &[&self.name]].concat()),
            UnitScope::Invalid(ref value) => Err(ResourceError::not_supported(format!(
                "unsupported systemd scope '{value}'"
            ))),
        }
    }

    fn apply_invocation(&self) -> ResourceResult<(&'static str, Vec<&str>)> {
        let action = if self.enabled { "enable" } else { "disable" };
        match self.scope {
            UnitScope::User => Ok(("systemctl", vec!["--user", action, "--now", &self.name])),
            UnitScope::System => Ok(("sudo", vec!["systemctl", action, "--now", &self.name])),
            UnitScope::Invalid(ref value) => Err(ResourceError::not_supported(format!(
                "unsupported systemd scope '{value}'"
            ))),
        }
    }

    fn state_from_is_enabled(&self, result: &crate::infra::exec::ExecResult) -> ResourceState {
        let output = command_output(result);
        if result.success {
            return if self.enabled {
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

    fn offline_enablement_links(&self) -> ResourceResult<Vec<(PathBuf, PathBuf)>> {
        let unit_path = self.offline_unit_path()?.ok_or_else(|| {
            anyhow::anyhow!(
                "user unit {} was not found in the unit search directories",
                self.name
            )
        })?;
        // Keep the installed path, even when it is a symlink into the checkout.
        // Uninstall materializes that path and may be followed by checkout removal.
        let source = unit_path.clone();
        let content = std::fs::read_to_string(&unit_path).map_err(|error| {
            anyhow::Error::new(error).context(format!("reading user unit {}", unit_path.display()))
        })?;
        let targets = install_targets(&content)?;
        if targets.is_empty() {
            return Err(ResourceError::not_supported(format!(
                "{} has no WantedBy or RequiredBy entries in [Install]",
                self.name
            )));
        }
        let user_dir = self.offline_user_dir()?;
        Ok(targets
            .into_iter()
            .map(|target| (user_dir.join(target).join(&self.name), source.clone()))
            .collect())
    }

    fn offline_user_dir(&self) -> ResourceResult<PathBuf> {
        let home = self.home.as_deref().ok_or_else(|| {
            ResourceError::not_supported("offline user-unit enablement requires a home directory")
        })?;
        Ok(home.join(".config/systemd/user"))
    }

    fn offline_unit_path(&self) -> ResourceResult<Option<PathBuf>> {
        let user_dir = self.offline_user_dir()?;
        for directory in std::iter::once(&user_dir).chain(&self.system_user_unit_dirs) {
            let candidate = directory.join(&self.name);
            // Do not bypass a mask or broken override in a higher-priority directory.
            match std::fs::symlink_metadata(&candidate) {
                Ok(_) => return Ok(Some(candidate)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(None)
    }

    fn offline_current_state(&self) -> ResourceResult<ResourceState> {
        if self.offline_unit_path()?.is_none() {
            // This is expected during a fresh dry run: the preceding symlink
            // task reports the unit definition it would install but does not
            // create it. The enablement link would therefore also be missing.
            return Ok(if self.enabled {
                ResourceState::Missing
            } else {
                ResourceState::Correct
            });
        }
        let links = self.offline_enablement_links()?;
        let mut all_enabled = true;
        let mut any_enabled = false;
        for (link, source) in &links {
            match std::fs::symlink_metadata(link) {
                Ok(_) if symlink_references(link, source) => any_enabled = true,
                Ok(_) if symlink_points_to(link, source) => {
                    any_enabled = true;
                    all_enabled = false;
                }
                Ok(_) => {
                    return Ok(ResourceState::Invalid {
                        reason: format!(
                            "{} conflicts with the expected systemd enablement link",
                            link.display()
                        ),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    all_enabled = false;
                }
                Err(error) => return Err(error.into()),
            }
        }
        if self.enabled {
            Ok(if all_enabled {
                ResourceState::Correct
            } else if any_enabled {
                ResourceState::Incorrect {
                    current: "enablement links are incomplete or bypass the installed unit"
                        .to_string(),
                }
            } else {
                ResourceState::Missing
            })
        } else if any_enabled {
            Ok(ResourceState::Incorrect {
                current: "enabled".to_string(),
            })
        } else {
            Ok(ResourceState::Correct)
        }
    }

    fn enable_offline(&self) -> ResourceResult<ResourceChange> {
        for (link, source) in self.offline_enablement_links()? {
            let parent = link
                .parent()
                .ok_or_else(|| anyhow::anyhow!("enablement link has no parent"))?;
            std::fs::create_dir_all(parent)?;
            match std::fs::symlink_metadata(&link) {
                Ok(_) if symlink_references(&link, &source) => continue,
                Ok(metadata) if metadata.file_type().is_symlink() => std::fs::remove_file(&link)?,
                Ok(_) => {
                    return Err(ResourceError::conflicting_state(
                        link.display().to_string(),
                        "systemd enablement symlink",
                        "non-symlink filesystem entry",
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            create_symlink(&source, &link)?;
        }
        Ok(ResourceChange::Applied)
    }

    fn disable_offline(&self) -> ResourceResult<ResourceChange> {
        for (link, source) in self.offline_enablement_links()? {
            match std::fs::symlink_metadata(&link) {
                Ok(_) if symlink_points_to(&link, &source) => std::fs::remove_file(&link)?,
                Ok(_) => {
                    return Err(ResourceError::conflicting_state(
                        link.display().to_string(),
                        "systemd enablement symlink",
                        "unexpected filesystem entry",
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(ResourceChange::Applied)
    }
}

fn runtime_state(enabled: bool, properties: &str) -> ResourceState {
    let property = |key: &str| {
        properties
            .lines()
            .filter_map(|line| line.split_once('='))
            .find_map(|(name, value)| (name == key).then_some(value))
    };
    let active = property("ActiveState").unwrap_or("");
    let completed_oneshot = active == "inactive"
        && property("Type") == Some("oneshot")
        && property("Result") == Some("success")
        && property("ExecMainStartTimestampMonotonic")
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|value| value > 0);
    let condition_skipped_oneshot = active == "inactive"
        && property("Type") == Some("oneshot")
        && property("Result") == Some("success")
        && property("ConditionResult") == Some("no");
    let matches = match active {
        "active" | "reloading" | "refreshing" | "activating" => enabled,
        "inactive" | "failed" => !enabled || completed_oneshot || condition_skipped_oneshot,
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

fn install_targets(content: &str) -> ResourceResult<Vec<String>> {
    let mut in_install = false;
    let mut targets = Vec::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_install = line == "[Install]";
            continue;
        }
        if !in_install || line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let suffix = match key.trim() {
            "WantedBy" => ".wants",
            "RequiredBy" => ".requires",
            _ => continue,
        };
        for target in value.split_whitespace() {
            let mut components = Path::new(target).components();
            if !matches!(components.next(), Some(std::path::Component::Normal(_)))
                || components.next().is_some()
            {
                return Err(ResourceError::not_supported(format!(
                    "invalid [Install] target {target:?}"
                )));
            }
            targets.push(format!("{target}{suffix}"));
        }
    }
    Ok(targets)
}

fn symlink_points_to(link: &Path, expected: &Path) -> bool {
    let Ok(actual) = std::fs::read_link(link) else {
        return false;
    };
    let resolved = if actual.is_absolute() {
        actual
    } else {
        link.parent().unwrap_or_else(|| Path::new(".")).join(actual)
    };
    resolved
        .canonicalize()
        .ok()
        .zip(expected.canonicalize().ok())
        .is_some_and(|(resolved_path, expected_path)| resolved_path == expected_path)
}

// Resolve parent components but preserve the final filename so a link to the
// installed unit differs from a link directly to its repository source.
fn symlink_references(link: &Path, expected: &Path) -> bool {
    fn reference_path(path: &Path) -> Option<PathBuf> {
        Some(path.parent()?.canonicalize().ok()?.join(path.file_name()?))
    }
    let Ok(actual) = std::fs::read_link(link) else {
        return false;
    };
    let resolved = link.parent().unwrap_or_else(|| Path::new(".")).join(actual);
    reference_path(&resolved)
        .zip(reference_path(expected))
        .is_some_and(|(resolved_path, expected_path)| resolved_path == expected_path)
}

#[cfg(unix)]
fn create_symlink(source: &Path, target: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

#[cfg(not(unix))]
fn create_symlink(_source: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "offline user-unit enablement requires Unix symlinks",
    ))
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
        if self.scope == UnitScope::User && !self.user_manager_available {
            return if self.enabled {
                self.enable_offline()
            } else {
                self.disable_offline()
            };
        }
        let (program, args) = self.apply_invocation()?;
        let action = if self.enabled { "enable" } else { "disable" };
        let result = self
            .executor
            .execute(CommandSpec::new(program).args(&args).unchecked())?;
        if result.success {
            Ok(ResourceChange::Applied)
        } else {
            Ok(ResourceChange::unusable(format!(
                "{program} failed to {action} {} ({}); stdout: {}; stderr: {}",
                self.name,
                exit_status(&result),
                output_if_present(result.stdout.trim()),
                output_if_present(result.stderr.trim())
            )))
        }
    }
}

impl IntrinsicState for SystemdUnitResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        if self.scope == UnitScope::User && !self.user_manager_available {
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
        let enablement = self.state_from_is_enabled(&result);
        // Missing disabled units need no runtime probe. Ambiguous enablement
        // must keep its diagnostic instead of being treated as converged.
        if !matches!(enablement, ResourceState::Correct)
            || (!self.enabled
                && (command_output(&result).contains("not-found")
                    || command_output(&result).contains("could not be found")))
        {
            return Ok(enablement);
        }
        let runtime_args = self.check_args(&[
            "show",
            "--property=ActiveState,Type,Result,ExecMainStartTimestampMonotonic,ConditionResult",
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
        Ok(runtime_state(self.enabled, &runtime_result.stdout))
    }
}

#[cfg(test)]
#[path = "tests/systemd_unit.rs"]
mod tests;
