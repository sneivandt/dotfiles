//! Systemd unit resource.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domains::system::config::systemd_units::UnitScope;
use crate::engine::resource::ResourceError;
use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::exec::{CommandSpec, Executor};

/// A systemd unit resource that can be checked and enabled.
#[derive(Debug)]
pub struct SystemdUnitResource {
    /// Unit name (e.g. "clean-home-tmp.timer").
    pub name: String,
    /// Systemd scope.
    pub scope: UnitScope,
    /// Executor for running systemctl commands.
    executor: Arc<dyn Executor>,
    /// Home directory containing user unit files.
    home: Option<PathBuf>,
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
            executor,
            home: None,
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
        resource.home = Some(home.to_path_buf());
        resource.user_manager_available = user_manager_available;
        resource
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

    fn offline_enablement_links(&self) -> ResourceResult<Vec<(PathBuf, PathBuf)>> {
        let unit_path = self.offline_unit_path()?;
        let source = unit_path.canonicalize().map_err(|error| {
            anyhow::Error::new(error)
                .context(format!("resolving user unit {}", unit_path.display()))
        })?;
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
        let user_dir = unit_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("user unit path has no parent"))?;
        Ok(targets
            .into_iter()
            .map(|target| (user_dir.join(target).join(&self.name), source.clone()))
            .collect())
    }

    fn offline_unit_path(&self) -> ResourceResult<PathBuf> {
        let home = self.home.as_deref().ok_or_else(|| {
            ResourceError::not_supported("offline user-unit enablement requires a home directory")
        })?;
        Ok(home
            .join(".config")
            .join("systemd")
            .join("user")
            .join(&self.name))
    }

    fn offline_current_state(&self) -> ResourceResult<ResourceState> {
        if !self.offline_unit_path()?.try_exists()? {
            // This is expected during a fresh dry run: the preceding symlink
            // task reports the unit definition it would install but does not
            // create it. The enablement link would therefore also be missing.
            return Ok(ResourceState::Missing);
        }
        let links = self.offline_enablement_links()?;
        if links
            .iter()
            .all(|(link, source)| symlink_points_to(link, source))
        {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Missing)
        }
    }

    fn enable_offline(&self) -> ResourceResult<ResourceChange> {
        for (link, source) in self.offline_enablement_links()? {
            let parent = link
                .parent()
                .ok_or_else(|| anyhow::anyhow!("enablement link has no parent"))?;
            std::fs::create_dir_all(parent)?;
            match std::fs::symlink_metadata(&link) {
                Ok(_) if symlink_points_to(&link, &source) => continue,
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
    resolved.canonicalize().is_ok_and(|path| path == expected)
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
        match self.scope {
            UnitScope::User => self.name.clone(),
            UnitScope::System => format!("{} (system scope)", self.name),
            UnitScope::Invalid(ref value) => {
                format!("{} (invalid '{value}' scope)", self.name)
            }
        }
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        if self.scope == UnitScope::User && !self.user_manager_available {
            return self.enable_offline();
        }
        let (program, args) = self.apply_invocation()?;
        let result = self
            .executor
            .execute(CommandSpec::new(program).args(&args).unchecked())?;
        if result.success {
            Ok(ResourceChange::Applied)
        } else {
            Ok(ResourceChange::unusable(format!(
                "{program} failed to enable {} ({}); stdout: {}; stderr: {}",
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
        let args = match self.check_args() {
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
        Ok(self.state_from_is_enabled(&result))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::infra::exec::{ExecResult, MockExecutor};

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
        };
        let resource =
            SystemdUnitResource::from_entry(&entry, executor, Path::new("/home/test"), true);
        assert_eq!(resource.name, "dunst.service");
        assert_eq!(resource.scope, UnitScope::User);
    }

    // ------------------------------------------------------------------
    // current_state
    // ------------------------------------------------------------------

    #[test]
    fn current_state_correct_when_systemctl_reports_enabled() {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .returning(|_| Ok(ExecResult::success("enabled\n")));
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

    #[test]
    fn offline_user_unit_missing_definition_is_plannable_for_dry_run() {
        let home = tempfile::tempdir().unwrap();
        let entry = crate::domains::system::config::systemd_units::SystemdUnit {
            name: "not-linked-yet.timer".to_string(),
            scope: UnitScope::User,
        };
        let resource = SystemdUnitResource::from_entry(
            &entry,
            Arc::new(MockExecutor::new()),
            home.path(),
            false,
        );

        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

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
