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
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::infra::exec::{ExecResult, MockExecutor};

    fn expect_runtime(mock: &mut MockExecutor, unit: &str, scope: UnitScope, output: &str) {
        let unit = unit.to_string();
        let output = output.to_string();
        mock.expect_execute()
            .once()
            .withf(move |spec| {
                let mut args = vec![
                    "show",
                    "--property=ActiveState,Type,Result,ExecMainStartTimestampMonotonic,ConditionResult",
                    &unit,
                ];
                if scope == UnitScope::User {
                    args.insert(0, "--user");
                }
                spec.program() == "systemctl"
                    && spec.arguments() == args.as_slice()
                    && !spec.is_checked()
            })
            .returning(move |_| Ok(ExecResult::success(output.clone())));
    }

    #[test]
    fn runtime_state_handles_daemons_and_completed_oneshots() {
        for (label, enabled, properties, correct) in [
            ("running daemon", true, "ActiveState=active", true),
            ("stopped daemon", true, "ActiveState=inactive", false),
            ("failed daemon", true, "ActiveState=failed", false),
            ("starting daemon", true, "ActiveState=activating", true),
            ("reloading daemon", true, "ActiveState=reloading", true),
            ("refreshing daemon", true, "ActiveState=refreshing", true),
            ("stopping daemon", true, "ActiveState=deactivating", false),
            (
                "disabled running daemon",
                false,
                "ActiveState=active",
                false,
            ),
            (
                "disabled starting daemon",
                false,
                "ActiveState=activating",
                false,
            ),
            (
                "disabled stopping daemon",
                false,
                "ActiveState=deactivating",
                false,
            ),
            (
                "disabled stopped daemon",
                false,
                "ActiveState=inactive",
                true,
            ),
            ("disabled failed daemon", false, "ActiveState=failed", true),
            (
                "completed oneshot",
                true,
                "ActiveState=inactive\nType=oneshot\nResult=success\nExecMainStartTimestampMonotonic=123",
                true,
            ),
            (
                "unstarted oneshot",
                true,
                "ActiveState=inactive\nType=oneshot\nResult=success\nExecMainStartTimestampMonotonic=0\nConditionResult=yes",
                false,
            ),
            (
                "condition-skipped oneshot",
                true,
                "ActiveState=inactive\nType=oneshot\nResult=success\nExecMainStartTimestampMonotonic=0\nConditionResult=no",
                true,
            ),
            (
                "failed oneshot",
                true,
                "ActiveState=failed\nType=oneshot\nResult=exit-code\nExecMainStartTimestampMonotonic=123",
                false,
            ),
            (
                "oneshot missing history",
                true,
                "ActiveState=inactive\nType=oneshot\nResult=success",
                false,
            ),
        ] {
            let state = runtime_state(enabled, properties);
            assert_eq!(
                matches!(state, ResourceState::Correct),
                correct,
                "{label}: {state:?}"
            );
            assert!(
                !matches!(state, ResourceState::Unknown { .. }),
                "{label}: {state:?}"
            );
        }
        for properties in ["", "ActiveState=unexpected"] {
            assert!(matches!(
                runtime_state(true, properties),
                ResourceState::Unknown { .. }
            ));
        }
    }

    #[test]
    fn runtime_probe_failure_is_not_converged() {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .withf(|spec| spec.arguments() == ["is-enabled", "test.service"])
            .returning(|_| Ok(ExecResult::success("enabled\n")));
        mock.expect_execute()
            .once()
            .withf(|spec| spec.arguments().first().is_some_and(|arg| arg == "show"))
            .returning(|_| Ok(ExecResult::failure("", "Failed to connect to bus", Some(1))));
        let resource = SystemdUnitResource::new("test.service", UnitScope::System, Arc::new(mock));
        assert!(
            matches!(resource.current_state().unwrap(), ResourceState::Unknown { reason } if reason.contains("Failed to connect to bus"))
        );
    }

    #[test]
    fn description_returns_unit_name() {
        let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::ProcessExecutor::system());
        let resource = SystemdUnitResource::new(
            "clean-home-tmp.timer".to_string(),
            UnitScope::User,
            executor,
        );
        assert_eq!(resource.description(), "clean-home-tmp.timer");
    }

    #[test]
    fn from_entry_copies_name() {
        let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::ProcessExecutor::system());
        let entry = crate::domains::system::config::systemd_units::SystemdUnit {
            name: "dunst.service".to_string(),
            scope: UnitScope::User,
            enabled: true,
        };
        let resource =
            SystemdUnitResource::from_entry(&entry, executor, Path::new("/home/test"), true);
        assert_eq!(resource.name, "dunst.service");
        assert_eq!(resource.scope, UnitScope::User);
        assert!(resource.enabled);
    }

    #[test]
    fn from_entry_copies_disabled_state() {
        let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::ProcessExecutor::system());
        let entry = crate::domains::system::config::systemd_units::SystemdUnit {
            name: "dhcpcd.service".to_string(),
            scope: UnitScope::System,
            enabled: false,
        };
        let resource =
            SystemdUnitResource::from_entry(&entry, executor, Path::new("/home/test"), true);
        assert!(!resource.enabled);
    }

    // ------------------------------------------------------------------
    // current_state
    // ------------------------------------------------------------------

    #[test]
    fn current_state_correct_when_systemctl_reports_enabled() {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .withf(|spec| spec.arguments().iter().any(|arg| arg == "is-enabled"))
            .returning(|_| Ok(ExecResult::success("enabled\n")));
        expect_runtime(
            &mut mock,
            "dunst.service",
            UnitScope::User,
            "ActiveState=active\n",
        );
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new("dunst.service", UnitScope::User, executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_missing_when_systemctl_reports_disabled() {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .returning(|_| Ok(ExecResult::failure("disabled\n", "", Some(1))));
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new("dunst.service", UnitScope::User, executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    #[test]
    fn current_state_correct_when_disabled_unit_is_disabled() {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .withf(|spec| spec.arguments().iter().any(|arg| arg == "is-enabled"))
            .returning(|_| Ok(ExecResult::failure("disabled\n", "", Some(1))));
        expect_runtime(
            &mut mock,
            "dhcpcd.service",
            UnitScope::System,
            "ActiveState=inactive\n",
        );
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let mut resource = SystemdUnitResource::new("dhcpcd.service", UnitScope::System, executor);
        resource.enabled = false;
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_incorrect_when_disabled_unit_is_enabled() {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .returning(|_| Ok(ExecResult::success("enabled\n")));
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let mut resource = SystemdUnitResource::new("dhcpcd.service", UnitScope::System, executor);
        resource.enabled = false;
        assert_eq!(
            resource.current_state().unwrap(),
            ResourceState::Incorrect {
                current: "enabled".to_string()
            }
        );
    }

    #[test]
    fn current_state_correct_when_disabled_unit_is_not_installed() {
        let mut mock = MockExecutor::new();
        mock.expect_execute().once().returning(|_| {
            Ok(ExecResult::failure(
                "not-found\n",
                "Unit dhcpcd.service could not be found.",
                Some(4),
            ))
        });
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let mut resource = SystemdUnitResource::new("dhcpcd.service", UnitScope::System, executor);
        resource.enabled = false;
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_missing_when_systemctl_reports_linked() {
        for state in ["linked", "linked-runtime"] {
            let state = state.to_string();
            let output = state.clone();
            let mut mock = MockExecutor::new();
            mock.expect_execute()
                .once()
                .returning(move |_| Ok(ExecResult::failure(format!("{output}\n"), "", Some(1))));
            let executor: Arc<dyn Executor> = Arc::new(mock);
            let resource = SystemdUnitResource::new(
                "network-manager-applet.service",
                UnitScope::User,
                executor,
            );
            assert_eq!(
                resource.current_state().unwrap(),
                ResourceState::Missing,
                "{state} units should be enabled"
            );
        }
    }

    #[test]
    fn current_state_unknown_when_systemctl_failure_is_ambiguous() {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .returning(|_| Ok(ExecResult::failure("", "Failed to connect to bus", Some(1))));
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
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "systemctl"
                    && spec.arguments() == ["is-enabled", "sshd.service"]
                    && !spec.is_checked()
            })
            .returning(|_| Ok(ExecResult::success("")));
        expect_runtime(
            &mut mock,
            "sshd.service",
            UnitScope::System,
            "ActiveState=active\n",
        );
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
        mock.expect_execute()
            .once()
            .returning(|_| Ok(ExecResult::success("")));
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new("dunst.service", UnitScope::User, executor);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[test]
    fn apply_returns_skipped_when_systemctl_fails() {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .returning(|_| Ok(ExecResult::failure("", "", Some(1))));
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
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "sudo"
                    && spec.arguments() == ["systemctl", "enable", "--now", "sshd.service"]
                    && !spec.is_checked()
            })
            .returning(|_| Ok(ExecResult::success("")));
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource = SystemdUnitResource::new("sshd.service", UnitScope::System, executor);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[test]
    fn apply_disables_system_scope_unit_with_sudo() {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "sudo"
                    && spec.arguments() == ["systemctl", "disable", "--now", "dhcpcd.service"]
                    && !spec.is_checked()
            })
            .returning(|_| Ok(ExecResult::success("")));
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let mut resource = SystemdUnitResource::new("dhcpcd.service", UnitScope::System, executor);
        resource.enabled = false;
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[cfg(unix)]
    #[test]
    fn offline_packaged_unit_enablement_uses_user_directory() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("home");
        let system_dir = fixture.path().join("usr/lib/systemd/user");
        std::fs::create_dir_all(&system_dir).unwrap();
        let unit = "gnome-keyring-daemon.socket";
        let source = system_dir.join(unit);
        std::fs::write(&source, "[Install]\nWantedBy=sockets.target\n").unwrap();
        let entry = crate::domains::system::config::systemd_units::SystemdUnit {
            name: unit.to_string(),
            scope: UnitScope::User,
            enabled: true,
        };
        let mut resource =
            SystemdUnitResource::from_entry(&entry, Arc::new(MockExecutor::new()), &home, false);
        resource.system_user_unit_dirs = vec![system_dir.clone()];
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
        assert!(!home.exists(), "discovery must not create user directories");
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        let link = home
            .join(".config/systemd/user/sockets.target.wants")
            .join(unit);
        assert_eq!(std::fs::read_link(&link).unwrap(), source);
        assert!(!system_dir.join("sockets.target.wants").exists());
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
        resource.enabled = false;
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Incorrect { .. }
        ));
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert!(!link.is_symlink());
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[cfg(unix)]
    #[test]
    fn offline_lookup_preserves_override_precedence_and_masks() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("home");
        let user_dir = home.join(".config/systemd/user");
        let admin_dir = fixture.path().join("etc/systemd/user");
        let package_dir = fixture.path().join("usr/lib/systemd/user");
        for dir in [&user_dir, &admin_dir, &package_dir] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(
                dir.join("example.service"),
                "[Install]\nWantedBy=default.target\n",
            )
            .unwrap();
        }
        let entry = crate::domains::system::config::systemd_units::SystemdUnit {
            name: "example.service".to_string(),
            scope: UnitScope::User,
            enabled: true,
        };
        let mut resource =
            SystemdUnitResource::from_entry(&entry, Arc::new(MockExecutor::new()), &home, false);
        resource.system_user_unit_dirs = vec![admin_dir.clone(), package_dir.clone()];
        for expected in [&user_dir, &admin_dir, &package_dir] {
            assert_eq!(
                resource.offline_unit_path().unwrap(),
                Some(expected.join(&entry.name))
            );
            std::fs::remove_file(expected.join(&entry.name)).unwrap();
        }
        std::fs::write(
            package_dir.join(&entry.name),
            "[Install]\nWantedBy=default.target\n",
        )
        .unwrap();
        for target in [Path::new("/dev/null"), &fixture.path().join("missing")] {
            let mask = user_dir.join(&entry.name);
            std::os::unix::fs::symlink(target, &mask).unwrap();
            assert_eq!(resource.offline_unit_path().unwrap(), Some(mask.clone()));
            assert!(
                resource.apply().is_err(),
                "must not fall back past a masked or broken override"
            );
            assert!(!user_dir.join("default.target.wants").exists());
            std::fs::remove_file(mask).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn offline_enablement_repairs_checkout_links_and_accepts_relative_installed_links() {
        let home = tempfile::tempdir().unwrap();
        let source = home.path().join("checkout/example.service");
        let unit_dir = home.path().join(".config/systemd/user");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::create_dir_all(unit_dir.join("default.target.wants")).unwrap();
        std::fs::write(&source, "[Install]\nWantedBy=default.target\n").unwrap();
        let installed = unit_dir.join("example.service");
        std::os::unix::fs::symlink(&source, &installed).unwrap();
        let link = unit_dir.join("default.target.wants/example.service");
        std::os::unix::fs::symlink(&source, &link).unwrap();
        let entry = crate::domains::system::config::systemd_units::SystemdUnit {
            name: "example.service".to_string(),
            scope: UnitScope::User,
            enabled: true,
        };
        let resource = SystemdUnitResource::from_entry(
            &entry,
            Arc::new(MockExecutor::new()),
            home.path(),
            false,
        );
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Incorrect { .. }
        ));
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(std::fs::read_link(&link).unwrap(), installed);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink("../example.service", &link).unwrap();
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[cfg(unix)]
    #[test]
    fn offline_user_unit_is_enabled_without_contacting_the_bus() {
        let home = tempfile::tempdir().unwrap();
        let unit_dir = home.path().join(".config/systemd/user");
        std::fs::create_dir_all(&unit_dir).unwrap();
        std::fs::write(
            unit_dir.join("clean-home-tmp.timer"),
            "[Unit]\nDescription=Clean tmp\n[Install]\nWantedBy=timers.target\n",
        )
        .unwrap();
        let entry = crate::domains::system::config::systemd_units::SystemdUnit {
            name: "clean-home-tmp.timer".to_string(),
            scope: UnitScope::User,
            enabled: true,
        };
        let resource = SystemdUnitResource::from_entry(
            &entry,
            Arc::new(MockExecutor::new()),
            home.path(),
            false,
        );

        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        let enabled = unit_dir.join("timers.target.wants/clean-home-tmp.timer");
        assert!(
            enabled.is_symlink(),
            "offline enablement should create a symlink"
        );
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[cfg(unix)]
    #[test]
    fn offline_user_unit_missing_definition_is_plannable_for_dry_run() {
        let home = tempfile::tempdir().unwrap();
        let entry = crate::domains::system::config::systemd_units::SystemdUnit {
            name: "not-linked-yet.timer".to_string(),
            scope: UnitScope::User,
            enabled: true,
        };
        let resource = SystemdUnitResource::from_entry(
            &entry,
            Arc::new(MockExecutor::new()),
            home.path(),
            false,
        );

        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    #[cfg(unix)]
    #[test]
    fn offline_user_unit_supports_required_by_targets() {
        let home = tempfile::tempdir().unwrap();
        let unit_dir = home.path().join(".config/systemd/user");
        std::fs::create_dir_all(&unit_dir).unwrap();
        std::fs::write(
            unit_dir.join("session.service"),
            "[Install]\nRequiredBy=graphical-session.target\n",
        )
        .unwrap();
        let entry = crate::domains::system::config::systemd_units::SystemdUnit {
            name: "session.service".to_string(),
            scope: UnitScope::User,
            enabled: true,
        };
        let resource = SystemdUnitResource::from_entry(
            &entry,
            Arc::new(MockExecutor::new()),
            home.path(),
            false,
        );

        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert!(
            unit_dir
                .join("graphical-session.target.requires/session.service")
                .is_symlink()
        );
    }

    #[cfg(unix)]
    #[test]
    fn offline_user_unit_can_be_disabled() {
        let home = tempfile::tempdir().unwrap();
        let unit_dir = home.path().join(".config/systemd/user");
        std::fs::create_dir_all(&unit_dir).unwrap();
        std::fs::write(
            unit_dir.join("session.service"),
            "[Install]\nWantedBy=graphical-session.target\n",
        )
        .unwrap();
        let entry = crate::domains::system::config::systemd_units::SystemdUnit {
            name: "session.service".to_string(),
            scope: UnitScope::User,
            enabled: true,
        };
        let resource = SystemdUnitResource::from_entry(
            &entry,
            Arc::new(MockExecutor::new()),
            home.path(),
            false,
        );
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);

        let disabled_entry = crate::domains::system::config::systemd_units::SystemdUnit {
            enabled: false,
            ..entry
        };
        let disabled = SystemdUnitResource::from_entry(
            &disabled_entry,
            Arc::new(MockExecutor::new()),
            home.path(),
            false,
        );
        assert!(matches!(
            disabled.current_state().unwrap(),
            ResourceState::Incorrect { .. }
        ));
        assert_eq!(disabled.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(disabled.current_state().unwrap(), ResourceState::Correct);
    }

    #[cfg(unix)]
    #[test]
    fn offline_user_unit_rejects_install_targets_that_escape_the_user_directory() {
        let home = tempfile::tempdir().unwrap();
        let unit_dir = home.path().join(".config/systemd/user");
        std::fs::create_dir_all(&unit_dir).unwrap();
        std::fs::write(
            unit_dir.join("unsafe.service"),
            "[Install]\nWantedBy=../../outside.target\n",
        )
        .unwrap();
        let entry = crate::domains::system::config::systemd_units::SystemdUnit {
            name: "unsafe.service".to_string(),
            scope: UnitScope::User,
            enabled: true,
        };
        let resource = SystemdUnitResource::from_entry(
            &entry,
            Arc::new(MockExecutor::new()),
            home.path(),
            false,
        );

        let error = resource
            .apply()
            .expect_err("escaping target must be rejected");
        assert!(error.to_string().contains("invalid [Install] target"));
        assert!(!home.path().join("outside.target.wants").exists());
    }
}
