//! Symlink resource.
use anyhow::Context as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domains::files::config::symlinks::{InspectedSource, inspect_source};
use crate::engine::{
    IntrinsicState, RemovableResource, Resource, ResourceChange, ResourceResult, ResourceState,
};
use crate::infra::exec::Executor;

mod materialize;
mod platform;
mod state;

#[cfg(test)]
use crate::infra::fs::resolve_directory_with;
use crate::infra::fs::{
    link_points_to_source, resolve_directory, resolve_target_entry, same_entry,
};

use materialize::copy_into_place;
use platform::{create_symlink, is_link_like};
use state::current_state;

/// A symlink resource that can be checked and applied.
#[derive(Debug, Clone)]
pub struct SymlinkResource {
    /// The source file/directory (what the symlink points to).
    pub source: PathBuf,
    /// The target path (where the symlink will be created).
    pub target: PathBuf,
    /// Executor used for subprocess fallbacks (e.g. mklink on Windows).
    executor: Arc<dyn Executor>,
    /// Configuration validation error that makes this resource unsafe to apply.
    validation_error: Option<String>,
    /// Home directory used to abbreviate user-facing target paths.
    display_home: Option<PathBuf>,
    allow_external_target: bool,
    /// Owning repository, used for source display and relocation checks.
    source_root: Option<PathBuf>,
    source_directory: Option<PathBuf>,
    other_root: Option<PathBuf>,
    restore_missing: bool,
    legacy_source: Option<PathBuf>,
}

impl SymlinkResource {
    /// Identify an installed link whose declared checkout entry disappeared.
    pub(crate) fn has_missing_managed_source(&self) -> anyhow::Result<bool> {
        let target = self.checked_target()?;
        let Some(metadata) = crate::infra::fs::symlink_metadata_optional(&target, "stat target")?
        else {
            return Ok(false);
        };
        if !metadata.file_type().is_symlink() {
            return Ok(false);
        }
        let linked_source = std::fs::read_link(&target)?;
        Ok(
            link_points_to_source(&target, &linked_source, &self.source)?
                && crate::infra::fs::symlink_metadata_optional(&self.source, "stat source")?
                    .is_none(),
        )
    }

    /// Create a new symlink resource.
    #[must_use]
    pub fn new(
        source: impl Into<PathBuf>,
        target: impl Into<PathBuf>,
        executor: Arc<dyn Executor>,
    ) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            executor,
            validation_error: None,
            display_home: None,
            allow_external_target: false,
            source_root: None,
            source_directory: None,
            other_root: None,
            restore_missing: false,
            legacy_source: None,
        }
    }

    /// Attach a configuration validation error to prevent unsafe application.
    #[must_use]
    pub fn with_validation_error(mut self, validation_error: Option<String>) -> Self {
        self.validation_error = validation_error;
        self
    }

    /// Attach the home and owning repository roots.
    #[must_use]
    pub fn with_display_roots(
        mut self,
        home: impl Into<PathBuf>,
        root: impl Into<PathBuf>,
    ) -> Self {
        self.display_home = Some(home.into());
        self.source_root = Some(root.into());
        self
    }

    /// Permit a target redirected outside home by an existing parent link.
    #[must_use]
    pub const fn with_external_target_allowed(mut self, allowed: bool) -> Self {
        self.allow_external_target = allowed;
        self
    }

    pub(crate) fn with_other_checkout(mut self, other_root: Option<PathBuf>) -> Self {
        self.other_root = other_root;
        self
    }

    pub(crate) fn with_source_directory(mut self, directory: PathBuf) -> Self {
        self.source_directory = Some(directory);
        self
    }

    fn inspect_source(&self) -> anyhow::Result<InspectedSource> {
        inspect_source(&self.source, self.source_directory.as_deref())
    }

    pub(crate) const fn with_restore_missing(mut self, restore_missing: bool) -> Self {
        self.restore_missing = restore_missing;
        self
    }

    pub(crate) fn with_legacy_source(mut self, source: Option<PathBuf>) -> Self {
        self.legacy_source = source;
        self
    }

    fn legacy_link(&self, target: &Path, existing: &Path) -> ResourceResult<Option<&Path>> {
        let Some(source) = self.legacy_source.as_deref() else {
            return Ok(None);
        };
        if link_points_to_source(target, existing, source)?
            && crate::infra::fs::symlink_metadata_optional(source, "stat legacy source")?.is_none()
        {
            return Ok(Some(source));
        }
        Ok(None)
    }

    fn managed_source(&self, target: &Path) -> ResourceResult<Option<&Path>> {
        let existing = std::fs::read_link(target)
            .with_context(|| format!("read link: {}", target.display()))?;
        if link_points_to_source(target, &existing, &self.source)? {
            return Ok(Some(&self.source));
        }
        self.legacy_link(target, &existing)
    }

    fn checkout_roots(&self) -> Vec<&Path> {
        self.source_root
            .as_deref()
            .into_iter()
            .chain(self.other_root.as_deref())
            .collect()
    }

    fn resolved_target(&self) -> anyhow::Result<(PathBuf, bool)> {
        let Some(home) = &self.display_home else {
            return Ok((self.target.clone(), false));
        };
        let home = resolve_directory(home)?;
        let resolved = resolve_target_entry(&self.target)?;
        let external = !resolved
            .parent()
            .is_some_and(|parent| parent.starts_with(home));
        Ok((resolved, external))
    }

    pub(crate) fn checked_target(&self) -> anyhow::Result<PathBuf> {
        let (resolved, external) = self.resolved_target()?;
        anyhow::ensure!(
            !external || self.allow_external_target,
            "target {} resolves outside home to {}; set allow_external_target = true to permit it",
            self.target.display(),
            resolved.display()
        );
        // A directory link from an older selection can redirect a child into
        // the checkout even when both the checkout and the target are in home.
        for root in self.checkout_roots() {
            let sources = resolve_directory(&root.join("symlinks"))?;
            anyhow::ensure!(
                !resolved.starts_with(sources),
                "target {} resolves into a managed source tree at {}; move the redirecting parent aside before retrying",
                self.target.display(),
                resolved.display()
            );
        }
        Ok(resolved)
    }

    fn display_target(&self) -> String {
        self.display_home
            .as_deref()
            .and_then(|home| self.target.strip_prefix(home).ok())
            .map_or_else(
                || display_path(&self.target),
                |relative| {
                    let relative = display_path(relative);
                    if relative.is_empty() {
                        "~".to_string()
                    } else {
                        format!("~/{relative}")
                    }
                },
            )
    }

    fn display_source(&self) -> String {
        self.source_root
            .as_deref()
            .and_then(|root| self.source.strip_prefix(root).ok())
            .map_or_else(|| display_path(&self.source), display_path)
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

impl Resource for SymlinkResource {
    /// Render as `target \u{2192} source`, matching every other resource's
    /// left-to-right `subject \u{2192} value` reading.
    fn description(&self) -> String {
        let target = self.display_target();
        let source = self.display_source();
        if let Ok((resolved, true)) = self.resolved_target() {
            return format!("{target} ({}) \u{2192} {source}", display_path(&resolved));
        }
        format!("{target} \u{2192} {source}")
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        let target = self.checked_target()?;
        materialize::check_recovery(&target)?;
        match current_state(self, &target)? {
            ResourceState::Correct => return Ok(ResourceChange::AlreadyCorrect),
            ResourceState::Missing => {}
            ResourceState::Incorrect { .. } => {
                let existing = std::fs::read_link(&target)?;
                let legacy = self.legacy_link(&target, &existing)?.with_context(|| {
                    format!(
                        "historical link changed while inspecting {}",
                        target.display()
                    )
                })?;
                materialize::replace_link(&self.source, &target, legacy, &*self.executor)?;
                return Ok(ResourceChange::Applied);
            }
            ResourceState::Invalid { reason } | ResourceState::Unknown { reason } => {
                return Err(crate::engine::resource::ResourceError::conflicting_state(
                    target.display().to_string(),
                    "an absent target or the correct managed link",
                    reason,
                ));
            }
        }
        crate::infra::fs::ensure_parent_dir(&target)?;
        create_symlink(&self.source, &target, &*self.executor)
            .with_context(|| format!("create link: {}", target.display()))?;
        Ok(ResourceChange::Applied)
    }
}

impl RemovableResource for SymlinkResource {
    fn removal_description(&self) -> String {
        format!("{} from {}", self.display_target(), self.display_source())
    }

    fn removal_state(&self) -> ResourceResult<ResourceState> {
        let target = match self.checked_target() {
            Ok(target) => target,
            Err(error) => {
                return Ok(ResourceState::Unknown {
                    reason: format!("{error:#}"),
                });
            }
        };
        materialize::check_recovery(&target)?;
        let metadata = crate::infra::fs::symlink_metadata_optional(&target, "stat target")?;
        if let Some(meta) = &metadata {
            let managed = is_link_like(&target, meta)
                && !same_entry(&self.source, &target)?
                && self.managed_source(&target)?.is_some();
            if !managed {
                return Ok(ResourceState::Incorrect {
                    current: "target is not a managed link".into(),
                });
            }
        }
        if metadata.is_none() && !self.restore_missing {
            return Ok(ResourceState::Missing);
        }
        if let Some(reason) = state::invalid_source_reason(self, &target)? {
            return Ok(ResourceState::Unknown { reason });
        }
        materialize::validate_source(&self.source, &self.checkout_roots())?;
        Ok(if metadata.is_some() {
            ResourceState::Correct
        } else {
            ResourceState::Missing
        })
    }

    fn remove_when_missing(&self) -> bool {
        self.restore_missing
    }

    fn remove(&self) -> ResourceResult<ResourceChange> {
        let target = self.checked_target()?;
        if self.validation_error.is_some() || same_entry(&self.source, &target)? {
            return Ok(ResourceChange::skipped(
                "invalid symlink will not be replaced with a copy",
            ));
        }
        let managed_source = match crate::infra::fs::symlink_metadata_optional(
            &target,
            "stat target",
        )? {
            Some(meta) if is_link_like(&target, &meta) => {
                self.managed_source(&target)?.with_context(|| {
                    format!(
                        "refusing to replace link that no longer points to the managed source: {}",
                        target.display()
                    )
                })?
            }
            Some(_) => {
                return Ok(ResourceChange::skipped(format!(
                    "target is not a symlink and will not be overwritten: {}",
                    target.display()
                )));
            }
            None if !self.restore_missing => return Ok(ResourceChange::AlreadyCorrect),
            None => &self.source,
        };

        // Stage source content before replacing the managed link.
        let roots = self.checkout_roots();
        let source = match self.inspect_source()? {
            InspectedSource::Available(path) => path,
            InspectedSource::Missing(reason) | InspectedSource::Outside(reason) => {
                return Err(anyhow::anyhow!("inspect copy source: {reason}").into());
            }
        };
        materialize::validate_source(&source, &roots)?;
        copy_into_place(
            &source,
            &target,
            managed_source,
            &roots,
            self.restore_missing,
        )
        .with_context(|| format!("copy {} to {}", self.source.display(), target.display()))?;
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for SymlinkResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        match self.checked_target() {
            Ok(target) => {
                if let Err(error) = materialize::check_recovery(&target) {
                    return Ok(ResourceState::Invalid {
                        reason: format!("{error:#}"),
                    });
                }
                current_state(self, &target)
            }
            Err(error) => Ok(ResourceState::Invalid {
                reason: format!("{error:#}"),
            }),
        }
    }
}

#[cfg(test)]
pub(super) fn copy_dir_into_place(
    source: &Path,
    target: &Path,
    repository: &Path,
) -> anyhow::Result<()> {
    materialize::copy_dir_into_place(source, target, source, &[repository], true)
}

#[cfg(test)]
pub(super) fn paths_equal(a: &Path, b: &Path) -> bool {
    crate::infra::fs::paths_equal(a, b).expect("compare fixture paths")
}

#[cfg(windows)]
#[cfg(test)]
pub(super) fn create_junction(
    target: &Path,
    link: &Path,
    executor: &dyn Executor,
) -> anyhow::Result<()> {
    platform::create_junction(target, link, executor)
}

#[cfg(test)]
mod resolution_tests {
    use super::*;

    #[test]
    fn directory_created_between_resolution_and_metadata_is_resolved() {
        let fixture = tempfile::tempdir().unwrap();
        let parent = fixture.path().join("concurrent-parent");
        let resolved = resolve_directory_with(&parent, |path| {
            let result = dunce::canonicalize(path);
            if result
                .as_ref()
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
            {
                std::fs::create_dir(path).unwrap();
            }
            result
        })
        .unwrap();
        assert_eq!(resolved, dunce::canonicalize(parent).unwrap());
    }
}
