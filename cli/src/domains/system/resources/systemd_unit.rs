//! Systemd unit resource.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domains::system::config::systemd_units::{UnitScope, safe_unit_name};
use crate::engine::resource::ResourceError;
use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::exec::{CommandSpec, Executor};
use crate::infra::provisioning::{
    DEFERRED_EXTENSIONS_MARKER, DeferredExtensionsState, deferred_extensions_state,
};

const THUNAR_SETTINGS_UNIT: &str = "thunar-view-settings.service";
const THUNAR_SETTINGS_SCRIPT: &str = ".config/Thunar/managed-view-settings.sh";
const FIRST_LOGIN_UNIT: &str = "dotfiles-first-login.service";

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
            user_manager_available: true,
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
        user_manager_available: bool,
        graphical_session_active: bool,
    ) -> Self {
        let mut resource = Self::new(entry.name.clone(), entry.scope.clone(), executor);
        resource.enabled = entry.enabled;
        resource.home = Some(home.to_path_buf());
        resource.user_manager_available = user_manager_available;
        resource.start = !entry.graphical || graphical_session_active;
        if entry.enabled && entry.scope == UnitScope::User && entry.name == THUNAR_SETTINGS_UNIT {
            resource.thunar_settings_script = Some(home.join(THUNAR_SETTINGS_SCRIPT));
        }
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
        if result.success && result.stdout.trim() == "enabled-runtime" {
            // Runtime enablement disappears at reboot; enable without --runtime
            // to establish the persistent links required by desired state.
            return if self.enabled {
                ResourceState::Missing
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
            if self.enabled && state == "indirect" {
                return ResourceState::Invalid {
                    reason: "indirect unit does not confirm persistent enablement; declare the unit with direct install links".into(),
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

    fn offline_enablement_links(&self) -> ResourceResult<Vec<(PathBuf, PathBuf)>> {
        if self.name.contains('@') {
            return Err(ResourceError::not_supported(
                "offline enablement of template units requires a live user systemd manager",
            ));
        }
        let unit_path = self.offline_unit_path()?.ok_or_else(|| {
            anyhow::anyhow!(
                "user unit {} was not found in the unit search directories",
                self.name
            )
        })?;
        // Keep the installed path, even when it is a symlink into the checkout.
        // Uninstall materializes that path and may be followed by checkout removal.
        let source = unit_path.clone();
        let content = self.offline_install_contents(&unit_path)?;
        let targets = install_targets(&content)?;
        if targets.is_empty() {
            return Err(ResourceError::not_supported(format!(
                "{} has no WantedBy or RequiredBy entries in [Install]",
                self.name
            )));
        }
        let user_dir = self.offline_user_dir()?;
        targets
            .into_iter()
            .map(|target| {
                let parent =
                    crate::infra::fs::checked_directory_chain(&user_dir, Path::new(&target))?;
                Ok((parent.join(&self.name), source.clone()))
            })
            .collect()
    }

    fn offline_user_dir(&self) -> ResourceResult<PathBuf> {
        let home = self.home.as_deref().ok_or_else(|| {
            ResourceError::not_supported("offline user-unit enablement requires a home directory")
        })?;
        Ok(crate::infra::fs::checked_directory_chain(
            home,
            Path::new(".config/systemd/user"),
        )?)
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

    // Support unit-specific drop-ins using native filename and search-path
    // precedence. Broader drop-ins with install metadata require native systemd;
    // silently ignoring them could enable a different relationship.
    fn offline_install_contents(&self, unit: &Path) -> ResourceResult<String> {
        let user_dir = self.offline_user_dir()?;
        let directories: Vec<_> = std::iter::once(&user_dir)
            .chain(&self.system_user_unit_dirs)
            .collect();
        let mut contents = std::fs::read_to_string(unit)?;
        let mut dropins = std::collections::BTreeMap::new();
        let (stem, suffix) = self
            .name
            .rsplit_once('.')
            .ok_or_else(|| anyhow::anyhow!("unit name has no suffix"))?;
        let mut broader = vec![format!("{suffix}.d")];
        broader.extend(
            stem.match_indices('-')
                .map(|(index, _)| format!("{}.{suffix}.d", stem.get(..=index).unwrap_or(stem))),
        );
        for directory in directories {
            for name in broader
                .iter()
                .chain(std::iter::once(&format!("{}.d", self.name)))
            {
                let entries = match std::fs::read_dir(directory.join(name)) {
                    Ok(entries) => entries,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error.into()),
                };
                for entry in entries {
                    let entry = entry?;
                    if entry
                        .path()
                        .extension()
                        .is_none_or(|extension| extension != "conf")
                    {
                        continue;
                    }
                    if name == &format!("{}.d", self.name) {
                        dropins
                            .entry(entry.file_name())
                            .or_insert_with(|| entry.path());
                    } else if has_install_directives(&std::fs::read_to_string(entry.path())?) {
                        return Err(ResourceError::not_supported(format!(
                            "offline enablement cannot interpret [Install] directives in {}; use a live user systemd manager",
                            entry.path().display()
                        )));
                    }
                }
            }
        }
        for path in dropins.into_values() {
            contents.push_str("\n[Unit]\n");
            contents.push_str(&std::fs::read_to_string(path)?);
        }
        Ok(contents)
    }

    fn offline_current_state(&self) -> ResourceResult<ResourceState> {
        if !self.enabled {
            return Ok(if self.offline_disable_links()?.is_empty() {
                ResourceState::Correct
            } else {
                ResourceState::Incorrect {
                    current: "enabled".to_string(),
                }
            });
        }
        if self.offline_unit_path()?.is_none() {
            // This is expected during a fresh dry run: the preceding symlink
            // task reports the unit definition it would install but does not
            // create it. The enablement link would therefore also be missing.
            return Ok(ResourceState::Missing);
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
        Ok(if all_enabled {
            ResourceState::Correct
        } else if any_enabled {
            ResourceState::Incorrect {
                current: "enablement links are incomplete or bypass the installed unit".to_string(),
            }
        } else {
            ResourceState::Missing
        })
    }

    fn enable_offline(&self) -> ResourceResult<ResourceChange> {
        for (link, source) in self.offline_enablement_links()? {
            let parent = link
                .parent()
                .ok_or_else(|| anyhow::anyhow!("enablement link has no parent"))?;
            std::fs::create_dir_all(parent)?;
            let user_dir = self.offline_user_dir()?;
            crate::infra::fs::checked_directory_chain(
                &user_dir,
                parent
                    .strip_prefix(&user_dir)
                    .map_err(anyhow::Error::from)?,
            )?;
            match std::fs::symlink_metadata(&link) {
                Ok(_) if symlink_references(&link, &source) => continue,
                Ok(metadata)
                    if metadata.file_type().is_symlink() && symlink_points_to(&link, &source) =>
                {
                    std::fs::remove_file(&link)?;
                }
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

    fn offline_disable_links(&self) -> ResourceResult<Vec<PathBuf>> {
        let user_dir = self.offline_user_dir()?;
        let directories = match std::fs::read_dir(&user_dir) {
            Ok(directories) => directories,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let sources: Vec<_> = std::iter::once(&user_dir)
            .chain(&self.system_user_unit_dirs)
            .map(|directory| directory.join(&self.name))
            .collect();
        let mut links = Vec::new();
        // Disable the installed relationships, not the unit's potentially
        // changed [Install] directives. Never traverse user-replaced directories.
        for directory in directories {
            let directory = directory?;
            let name = directory.file_name();
            let name = name.to_string_lossy();
            if !(name.ends_with(".wants") || name.ends_with(".requires")) {
                continue;
            }
            if !directory.file_type()?.is_dir() {
                return Err(ResourceError::conflicting_state(
                    directory.path().display().to_string(),
                    "systemd enablement directory",
                    "non-directory filesystem entry",
                ));
            }
            let link = directory.path().join(&self.name);
            match std::fs::symlink_metadata(&link) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    if sources.iter().any(|source| {
                        symlink_references(&link, source) || symlink_points_to(&link, source)
                    }) {
                        links.push(link);
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(links)
    }

    fn disable_offline(&self) -> ResourceResult<ResourceChange> {
        let links = self.offline_disable_links()?;
        if links.is_empty() {
            return Ok(ResourceChange::AlreadyCorrect);
        }
        for link in links {
            let user_dir = self.offline_user_dir()?;
            let parent = link
                .parent()
                .ok_or_else(|| anyhow::anyhow!("enablement link has no parent"))?;
            crate::infra::fs::checked_directory_chain(
                &user_dir,
                parent
                    .strip_prefix(&user_dir)
                    .map_err(anyhow::Error::from)?,
            )?;
            if !self.offline_disable_links()?.contains(&link) {
                return Err(anyhow::anyhow!(
                    "systemd enablement link changed during removal; retry"
                )
                .into());
            }
            std::fs::remove_file(link)?;
        }
        Ok(ResourceChange::Applied)
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

fn install_targets(content: &str) -> ResourceResult<Vec<String>> {
    let mut in_install = false;
    let mut targets: Vec<String> = Vec::new();
    let mut lines = content.lines();
    while let Some(raw_line) = lines.next() {
        let mut line = raw_line.to_string();
        if line.trim().is_empty() || line.trim_start().starts_with(['#', ';']) {
            continue;
        }
        // Comments are skipped during continuation, but blank lines end it.
        while line.bytes().rev().take_while(|byte| *byte == b'\\').count() % 2 == 1 {
            line.pop();
            line.push(' ');
            let Some(next) = lines.find(|next| !next.trim_start().starts_with(['#', ';'])) else {
                break;
            };
            line.push_str(next);
        }
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_install = line == "[Install]";
            continue;
        }
        if !in_install {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let suffix = match key.trim() {
            "WantedBy" => ".wants",
            "RequiredBy" => ".requires",
            _ if value.trim().is_empty() => continue,
            key => {
                return Err(ResourceError::not_supported(format!(
                    "offline enablement cannot interpret [Install] {key}; use a live user systemd manager"
                )));
            }
        };
        if value.trim().is_empty() {
            targets.retain(|target| !target.ends_with(suffix));
            continue;
        }
        for target in value.split_whitespace() {
            if !safe_unit_name(target) {
                return Err(ResourceError::not_supported(format!(
                    "invalid [Install] target {target:?}"
                )));
            }
            targets.push(format!("{target}{suffix}"));
        }
    }
    Ok(targets)
}

fn has_install_directives(content: &str) -> bool {
    let mut in_install = false;
    content.lines().any(|line| {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_install = line == "[Install]";
        }
        in_install && !line.starts_with(['#', ';']) && line.contains('=')
    })
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
        if self.scope == UnitScope::User && !self.user_manager_available {
            return if self.enabled {
                self.enable_offline()
            } else {
                self.disable_offline()
            };
        }
        if self.enabled {
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
        if result.success && result.stdout.trim() == "alias" {
            return self.alias_state();
        }
        let enablement = self.state_from_is_enabled(&result);
        // Missing disabled units need no runtime probe. Ambiguous enablement
        // must keep its diagnostic instead of being treated as converged.
        if !matches!(enablement, ResourceState::Correct)
            || (self.enabled && !self.start)
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
        let properties = &runtime_result.stdout;
        // The first-login service removes its marker after finishing deferred work.
        // Start history can be stale or lost when the user manager restarts.
        if self.enabled
            && self.scope == UnitScope::User
            && self.name == FIRST_LOGIN_UNIT
            && systemd_property(properties, "ActiveState") == Some("inactive")
            && let Some(home) = &self.home
        {
            let marker = home.join(DEFERRED_EXTENSIONS_MARKER);
            return Ok(match deferred_extensions_state(&marker)? {
                DeferredExtensionsState::Absent => ResourceState::Correct,
                DeferredExtensionsState::Pending => ResourceState::Incorrect {
                    current: "VS Code extension provisioning is pending".into(),
                },
                DeferredExtensionsState::Invalid => ResourceState::Invalid {
                    reason: format!("{} is not a regular file", marker.display()),
                },
            });
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
