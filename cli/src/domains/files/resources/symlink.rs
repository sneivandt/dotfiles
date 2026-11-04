//! Symlink resource.
use anyhow::Context as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::engine::{
    IntrinsicState, RemovableResource, Resource, ResourceChange, ResourceResult, ResourceState,
};
use crate::infra::exec::Executor;

mod materialize;
mod platform;
mod state;

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
    other_root: Option<PathBuf>,
    restore_missing: bool,
}

impl SymlinkResource {
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
            other_root: None,
            restore_missing: false,
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

    pub(crate) const fn with_restore_missing(mut self, restore_missing: bool) -> Self {
        self.restore_missing = restore_missing;
        self
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

/// Resolve the parents of a target while preserving its final entry.
pub(crate) fn resolve_target_entry(path: &Path) -> anyhow::Result<PathBuf> {
    let parent = path.parent().context("target has no parent")?;
    let name = path.file_name().context("target has no file name")?;
    Ok(resolve_directory(parent)?.join(name))
}

// Resolve existing parents without following the final target entry. A dangling
// parent link is an error rather than a missing directory we can create.
fn resolve_directory(path: &Path) -> anyhow::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    for ancestor in absolute.ancestors() {
        match dunce::canonicalize(ancestor) {
            Ok(resolved) => return Ok(resolved.join(absolute.strip_prefix(ancestor)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if crate::infra::fs::symlink_metadata_optional(ancestor, "inspect target parent")?
                    .is_none()
                {
                    continue;
                }
                return Err(error)
                    .with_context(|| format!("resolve target parent {}", ancestor.display()));
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("resolve target parent {}", ancestor.display()));
            }
        }
    }
    anyhow::bail!("cannot resolve target parent {}", path.display())
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
        match current_state(self, &target)? {
            ResourceState::Correct => return Ok(ResourceChange::AlreadyCorrect),
            ResourceState::Missing => {}
            ResourceState::Invalid { reason }
            | ResourceState::Unknown { reason }
            | ResourceState::Incorrect { current: reason } => {
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
                && !state::same_entry(&self.source, &target)
                && state::link_points_to_source(
                    &target,
                    &std::fs::read_link(&target)
                        .with_context(|| format!("read link: {}", target.display()))?,
                    &self.source,
                );
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
        if self.validation_error.is_some() || state::same_entry(&self.source, &target) {
            return Ok(ResourceChange::skipped(
                "invalid symlink resource will not be materialized",
            ));
        }
        match crate::infra::fs::symlink_metadata_optional(&target, "stat target")? {
            Some(meta) if is_link_like(&target, &meta) => {}
            Some(_) => {
                return Ok(ResourceChange::skipped(format!(
                    "target is not a symlink and will not be overwritten: {}",
                    target.display()
                )));
            }
            None if !self.restore_missing => return Ok(ResourceChange::AlreadyCorrect),
            None => {}
        }

        // Stage source content before replacing the managed link.
        let roots = self.checkout_roots();
        materialize::validate_source(&self.source, &roots)?;
        copy_into_place(&self.source, &target, &roots, self.restore_missing).with_context(
            || {
                format!(
                    "materialize {} to {}",
                    self.source.display(),
                    target.display()
                )
            },
        )?;
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for SymlinkResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        match self.checked_target() {
            Ok(target) => current_state(self, &target),
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
    materialize::copy_dir_into_place(source, target, &[repository], true)
}

#[cfg(test)]
pub(super) fn paths_equal(a: &Path, b: &Path) -> bool {
    state::paths_equal(a, b)
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
