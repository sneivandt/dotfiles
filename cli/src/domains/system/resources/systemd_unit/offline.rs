use super::install_targets::{has_install_directives, install_targets};
use super::{
    Path, PathBuf, ResourceChange, ResourceError, ResourceResult, ResourceState,
    SystemdUnitResource, UnitScope,
};

impl SystemdUnitResource {
    pub(super) fn offline_enablement_links(&self) -> ResourceResult<Vec<(PathBuf, PathBuf)>> {
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

    pub(super) fn offline_user_dir(&self) -> ResourceResult<PathBuf> {
        let home = self.home.as_deref().ok_or_else(|| {
            ResourceError::not_supported("offline user-unit enablement requires a home directory")
        })?;
        Ok(crate::infra::fs::checked_directory_chain(
            home,
            Path::new(".config/systemd/user"),
        )?)
    }

    pub(super) fn offline_unit_path(&self) -> ResourceResult<Option<PathBuf>> {
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
        // A mask takes precedence over install directives in all drop-ins.
        // Refuse non-regular inputs before opening them (e.g. /dev/null).
        let metadata = std::fs::metadata(unit)?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(ResourceError::conflicting_state(
                unit.display().to_string(),
                "an unmasked unit file",
                "an empty or non-regular file",
            ));
        }
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

    pub(super) fn offline_current_state(&self) -> ResourceResult<ResourceState> {
        if !self.enabled {
            if let Some(state) = self.global_enablement_conflict()? {
                return Ok(state);
            }
            return Ok(if self.offline_enabled_links()?.is_empty() {
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
        self.enablement_link_state(&self.offline_enablement_links()?, false)
    }

    pub(super) fn live_user_link_state(&self) -> ResourceResult<Option<ResourceState>> {
        if !self.enabled || self.scope != UnitScope::User || self.home.is_none() {
            return Ok(None);
        }
        let unit = self.offline_user_dir()?.join(&self.name);
        match std::fs::metadata(unit) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => return Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        }
        if self.offline_enabled_links()?.is_empty() {
            return Ok(None);
        }
        let links = match self.offline_enablement_links() {
            Ok(links) => links,
            // Keep native enablement for aliases, templates and directives
            // the per-user link reconciler cannot model.
            Err(ResourceError::NotSupported { .. }) => return Ok(None),
            Err(error) => return Err(error),
        };
        Ok(Some(self.enablement_link_state(&links, true)?))
    }

    fn enablement_link_state(
        &self,
        links: &[(PathBuf, PathBuf)],
        accept_direct_links: bool,
    ) -> ResourceResult<ResourceState> {
        let mut all_enabled = true;
        let mut any_enabled = false;
        for (link, source) in links {
            match std::fs::symlink_metadata(link) {
                Ok(_)
                    if symlink_references(link, source)
                        || (accept_direct_links && symlink_points_to(link, source)) =>
                {
                    any_enabled = true;
                }
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
        Ok(if !self.offline_obsolete_links(links)?.is_empty() {
            ResourceState::Incorrect {
                current: "obsolete enablement links remain".into(),
            }
        } else if all_enabled {
            ResourceState::Correct
        } else if any_enabled {
            ResourceState::Incorrect {
                current: "enablement links are incomplete or bypass the installed unit".to_string(),
            }
        } else {
            ResourceState::Missing
        })
    }

    pub(super) fn enable_offline(&self) -> ResourceResult<ResourceChange> {
        let links = self.offline_enablement_links()?;
        self.reconcile_user_links(&links, false)
    }

    pub(super) fn reconcile_user_links(
        &self,
        links: &[(PathBuf, PathBuf)],
        accept_direct_links: bool,
    ) -> ResourceResult<ResourceChange> {
        let obsolete = self.offline_obsolete_links(links)?;
        for (link, source) in links {
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
            match std::fs::symlink_metadata(link) {
                Ok(_)
                    if symlink_references(link, source)
                        || (accept_direct_links && symlink_points_to(link, source)) =>
                {
                    continue;
                }
                Ok(metadata)
                    if metadata.file_type().is_symlink() && symlink_points_to(link, source) =>
                {
                    std::fs::remove_file(link)?;
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
            create_symlink(source, link)?;
        }
        self.remove_offline_links(obsolete)?;
        Ok(ResourceChange::Applied)
    }

    fn offline_obsolete_links(
        &self,
        desired: &[(PathBuf, PathBuf)],
    ) -> ResourceResult<Vec<PathBuf>> {
        let mut links = self.offline_enabled_links()?;
        links.retain(|link| !desired.iter().any(|(wanted, _)| wanted == link));
        Ok(links)
    }

    fn offline_enabled_links(&self) -> ResourceResult<Vec<PathBuf>> {
        self.enablement_links_in(&self.offline_user_dir()?)
    }

    pub(super) fn global_enablement_conflict(&self) -> ResourceResult<Option<ResourceState>> {
        for directory in &self.system_user_unit_dirs {
            if let Some(link) = self.enablement_links_in(directory)?.first() {
                return Ok(Some(ResourceState::Invalid {
                    reason: format!(
                        "{} is enabled for all users by {}; ask an administrator to disable it",
                        self.name,
                        link.display()
                    ),
                }));
            }
        }
        Ok(None)
    }

    fn enablement_links_in(&self, unit_dir: &Path) -> ResourceResult<Vec<PathBuf>> {
        let directories = match std::fs::read_dir(unit_dir) {
            Ok(directories) => directories,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let user_dir = self.offline_user_dir()?;
        let sources: Vec<_> = std::iter::once(&user_dir)
            .chain(&self.system_user_unit_dirs)
            .map(|directory| directory.join(&self.name))
            .collect();
        let mut links = Vec::new();
        // Discover installed relationships independently of the unit's current
        // [Install] directives. Never traverse user-replaced directories.
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

    pub(super) fn disable_offline(&self) -> ResourceResult<ResourceChange> {
        if let Some(ResourceState::Invalid { reason }) = self.global_enablement_conflict()? {
            return Ok(ResourceChange::unusable(reason));
        }
        let links = self.offline_enabled_links()?;
        if links.is_empty() {
            return Ok(ResourceChange::AlreadyCorrect);
        }
        self.remove_offline_links(links)?;
        Ok(ResourceChange::Applied)
    }

    fn remove_offline_links(&self, links: Vec<PathBuf>) -> ResourceResult<()> {
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
            if !self.offline_enabled_links()?.contains(&link) {
                return Err(anyhow::anyhow!(
                    "systemd enablement link changed during removal; retry"
                )
                .into());
            }
            std::fs::remove_file(link)?;
        }
        Ok(())
    }
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
pub(super) fn symlink_references(link: &Path, expected: &Path) -> bool {
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
