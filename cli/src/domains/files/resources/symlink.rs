//! Symlink resource.
use anyhow::{Context as _, Result};
use std::path::PathBuf;
use std::sync::Arc;

use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::exec::Executor;

mod materialize;
mod platform;
mod state;

use materialize::copy_into_place;
use platform::{create_symlink, is_link_like, remove_symlink};
use state::{current_state, pre_apply_warning};

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
    /// Repository root used to abbreviate user-facing source paths.
    display_root: Option<PathBuf>,
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
            display_root: None,
        }
    }

    /// Attach a configuration validation error to prevent unsafe application.
    #[must_use]
    pub fn with_validation_error(mut self, validation_error: Option<String>) -> Self {
        self.validation_error = validation_error;
        self
    }

    /// Attach roots used to render concise user-facing paths.
    #[must_use]
    pub fn with_display_roots(
        mut self,
        home: impl Into<PathBuf>,
        root: impl Into<PathBuf>,
    ) -> Self {
        self.display_home = Some(home.into());
        self.display_root = Some(root.into());
        self
    }

    fn display_target(&self) -> String {
        self.display_home
            .as_deref()
            .and_then(|home| self.target.strip_prefix(home).ok())
            .map_or_else(
                || self.target.display().to_string(),
                |relative| PathBuf::from("~").join(relative).display().to_string(),
            )
    }

    fn display_source(&self) -> String {
        self.display_root
            .as_deref()
            .and_then(|root| self.source.strip_prefix(root).ok())
            .map_or_else(
                || self.source.display().to_string(),
                |relative| relative.display().to_string(),
            )
    }
}

impl Resource for SymlinkResource {
    fn description(&self) -> String {
        format!(
            "{} \u{2190} {}",
            self.display_target(),
            self.display_source()
        )
    }

    fn pre_apply_warning(&self) -> ResourceResult<Option<String>> {
        pre_apply_warning(&self.target)
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        crate::infra::fs::ensure_parent_dir(&self.target)?;

        // Attempt to remove any existing target; ignore NotFound since the
        // path may already be absent.  This avoids a TOCTOU race between a
        // separate existence check and the removal.
        match remove_symlink(&self.target, &*self.executor) {
            Ok(()) => {}
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
            {
                // Path was already absent — nothing to remove.
            }
            Err(e) => {
                return Err(e
                    .context(format!("remove existing: {}", self.target.display()))
                    .into());
            }
        }

        // Create the symlink
        create_symlink(&self.source, &self.target, &*self.executor)
            .with_context(|| format!("create link: {}", self.target.display()))?;

        Ok(ResourceChange::Applied)
    }

    fn remove(&self) -> ResourceResult<ResourceChange> {
        // Classify the target explicitly so that unexpected metadata errors
        // do not fall through to `copy_into_place` (which would materialize
        // the source into place — the wrong thing to do for a transient
        // stat failure).  A missing target is fine: we still materialize so
        // the user retains the file/directory after uninstall.
        match crate::infra::fs::symlink_metadata_optional(&self.target, "stat target")? {
            Some(meta) if is_link_like(&self.target, &meta) => {
                // Proceed with the normal materialize-then-remove path below.
            }
            Some(_) => {
                // Target exists but is not a symlink: refuse to overwrite to
                // protect user data that replaced the managed symlink.
                return Ok(ResourceChange::Skipped {
                    reason: format!(
                        "target is not a symlink and will not be overwritten: {}",
                        self.target.display()
                    ),
                });
            }
            None => {
                // Target already absent: still materialize source content into
                // place so the user ends up with the real file/directory after
                // uninstall, matching the behaviour when the symlink is present.
            }
        }

        // Copy source content into place, then remove the symlink, so the user
        // retains the file/directory after uninstall instead of losing it.
        copy_into_place(&self.source, &self.target, &*self.executor).with_context(|| {
            format!(
                "materialize {} to {}",
                self.source.display(),
                self.target.display()
            )
        })?;
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for SymlinkResource {
    fn current_state(&self) -> Result<ResourceState> {
        current_state(self)
    }
}

#[cfg(test)]
pub(super) fn sibling_temp_path(target: &std::path::Path, suffix: &str) -> PathBuf {
    materialize::sibling_temp_path(target, suffix)
}

#[cfg(test)]
pub(super) fn copy_dir_into_place(
    source: &std::path::Path,
    target: &std::path::Path,
    executor: &dyn Executor,
) -> Result<()> {
    materialize::copy_dir_into_place(source, target, executor)
}

#[cfg(test)]
pub(super) fn paths_equal(a: &std::path::Path, b: &std::path::Path) -> bool {
    state::paths_equal(a, b)
}

#[cfg(windows)]
#[cfg(test)]
pub(super) fn create_junction(
    target: &std::path::Path,
    link: &std::path::Path,
    executor: &dyn Executor,
) -> Result<()> {
    platform::create_junction(target, link, executor)
}
