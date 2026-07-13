//! Symlink resource.
use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::runtime::exec::Executor;

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
        }
    }

    /// Attach a configuration validation error to prevent unsafe application.
    #[must_use]
    pub fn with_validation_error(mut self, validation_error: Option<String>) -> Self {
        self.validation_error = validation_error;
        self
    }
}

impl Resource for SymlinkResource {
    fn description(&self) -> String {
        format!("{} -> {}", self.target.display(), self.source.display())
    }

    fn pre_apply_warning(&self) -> ResourceResult<Option<String>> {
        let metadata = crate::runtime::fs::symlink_metadata_optional(&self.target, "stat target")?;
        Ok(metadata
            .filter(|meta| !is_link_like(&self.target, meta))
            .map(|_| {
                format!(
                    "replacing existing non-symlink target without backup: {}",
                    self.target.display()
                )
            }))
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        crate::runtime::fs::ensure_parent_dir(&self.target)?;

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
        match crate::runtime::fs::symlink_metadata_optional(&self.target, "stat target")? {
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
        if let Some(reason) = &self.validation_error {
            return Ok(ResourceState::Invalid {
                reason: reason.clone(),
            });
        }

        if let Some(reason) = crate::runtime::fs::missing_source_reason(&self.source) {
            return Ok(ResourceState::Invalid { reason });
        }

        // Check if symlink already points to the correct source
        std::fs::read_link(&self.target).map_or_else(
            |_| match crate::runtime::fs::symlink_metadata_optional(&self.target, "stat target")? {
                Some(_) => Ok(ResourceState::Incorrect {
                    current: "target is a regular file or dangling symlink".to_string(),
                }),
                None => Ok(ResourceState::Missing),
            },
            |existing| {
                if paths_equal(&existing, &self.source) {
                    Ok(ResourceState::Correct)
                } else {
                    Ok(ResourceState::Incorrect {
                        current: format!("points to {}", existing.display()),
                    })
                }
            },
        )
    }
}

/// Copy `source` into `target`, replacing the symlink that currently lives at
/// `target`.  Files are staged to a sibling temp path first so that the window
/// where `target` is absent is as small as possible.  Directories are handled
/// recursively via [`crate::runtime::fs::copy_dir_recursive`]; symlinks within the
/// source tree are recreated as symlinks rather than followed, preventing
/// unintended traversal outside the source tree.
fn copy_into_place(source: &Path, target: &Path, executor: &dyn Executor) -> Result<()> {
    if source.is_dir() {
        copy_dir_into_place(source, target, executor)
    } else {
        copy_file_into_place(source, target, executor)
    }
}

/// Build a sibling temporary path by appending `suffix` to the target name.
pub(super) fn sibling_temp_path(target: &Path, suffix: &str) -> PathBuf {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target.file_name().map_or_else(
        || "dotfiles_tmp".to_string(),
        |n| format!("{}{suffix}", n.to_string_lossy()),
    );
    parent.join(name)
}

/// Copy a regular file: stage to a temp sibling, remove the symlink, rename
/// the temp file into place.
fn copy_file_into_place(source: &Path, target: &Path, executor: &dyn Executor) -> Result<()> {
    let tmp = sibling_temp_path(target, ".dotfiles_tmp");
    crate::runtime::fs::copy_file(source, &tmp)?;

    let mut guard = crate::runtime::fs::TempPath::new(tmp.clone());

    match crate::runtime::fs::symlink_metadata_optional(target, "stat target")? {
        Some(meta) if is_link_like(target, &meta) => {
            remove_symlink(target, executor)
                .with_context(|| format!("remove symlink: {}", target.display()))?;
        }
        Some(_) => {
            // Target exists but is not a symlink — refuse to overwrite to
            // prevent data loss.
            return Err(anyhow::anyhow!(
                "refusing to overwrite non-symlink target: {}",
                target.display()
            ));
        }
        None => {
            // Target is absent; nothing to remove before rename.
        }
    }

    std::fs::rename(&tmp, target)
        .with_context(|| format!("rename {} to {}", tmp.display(), target.display()))?;

    guard.persist();
    Ok(())
}

/// Copy a directory: stage into a sibling temp directory, remove the
/// symlink/junction, then rename the temp directory into place.  Falls back to
/// a plain copy+delete when the rename crosses a filesystem boundary (EXDEV).
pub(super) fn copy_dir_into_place(
    source: &Path,
    target: &Path,
    executor: &dyn Executor,
) -> Result<()> {
    let tmp = sibling_temp_path(target, "_dotfiles_tmp");
    remove_stale_temp_dir(&tmp)?;
    let mut guard = crate::runtime::fs::TempDir::new(tmp.clone());

    crate::runtime::fs::copy_dir_recursive(source, &tmp, false)
        .with_context(|| format!("recursive copy {} to {}", source.display(), tmp.display()))?;

    match crate::runtime::fs::symlink_metadata_optional(target, "stat target")? {
        Some(meta) if is_link_like(target, &meta) => {
            remove_symlink(target, executor)
                .with_context(|| format!("remove symlink/junction: {}", target.display()))?;
        }
        Some(_) => {
            // Target exists but is not a symlink — refuse to overwrite to
            // prevent data loss.
            return Err(anyhow::anyhow!(
                "refusing to overwrite non-symlink target: {}",
                target.display()
            ));
        }
        None => {
            // Target is absent; nothing to remove before rename.
        }
    }

    // Prefer atomic rename; fall back to copy+delete only on cross-filesystem move.
    match std::fs::rename(&tmp, target) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            crate::runtime::fs::copy_dir_recursive(&tmp, target, false).with_context(|| {
                format!("cross-fs copy {} to {}", tmp.display(), target.display())
            })?;
            // Disarm the guard before cleanup so it doesn't try to remove on drop.
            guard.persist();
            // Best-effort cleanup; failure here is non-fatal since target is correct.
            if let Err(cleanup_error) = std::fs::remove_dir_all(&tmp) {
                tracing::debug!(
                    "best-effort cleanup of {} failed: {cleanup_error}",
                    tmp.display()
                );
            }
        }
        Err(e) => {
            return Err(anyhow::Error::new(e).context(format!(
                "rename {} to {}",
                tmp.display(),
                target.display()
            )));
        }
    }

    guard.persist();
    Ok(())
}

fn remove_stale_temp_dir(tmp: &Path) -> Result<()> {
    let Some(meta) = crate::runtime::fs::symlink_metadata_optional(tmp, "stat temp path")? else {
        return Ok(());
    };

    if meta.file_type().is_symlink() || !meta.is_dir() {
        std::fs::remove_file(tmp)
            .with_context(|| format!("remove stale temp file: {}", tmp.display()))?;
    } else {
        std::fs::remove_dir_all(tmp)
            .with_context(|| format!("remove stale temp dir: {}", tmp.display()))?;
    }
    Ok(())
}

/// Compare two paths for equality, canonicalizing when possible.
///
/// Attempts `fs::canonicalize` on both paths so that symlinks in the path,
/// case differences (Windows), and `\\?\` UNC prefixes are resolved before
/// comparison.  Falls back to raw comparison when canonicalization fails
/// (e.g., dangling paths).
pub(super) fn paths_equal(a: &Path, b: &Path) -> bool {
    // Fast path: literal match (covers the common case where the symlink was
    // created by this tool and nothing has changed).
    if a == b {
        return true;
    }

    // Try canonicalizing both; fall back to original path on failure.
    let canon_a = std::fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let canon_b = std::fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());

    #[cfg(windows)]
    {
        // Windows filesystems are case-insensitive; compare with
        // case-folded Unicode to avoid false mismatches.
        let sa = canon_a.to_string_lossy().to_lowercase();
        let sb = canon_b.to_string_lossy().to_lowercase();
        sa == sb
    }

    #[cfg(not(windows))]
    {
        canon_a == canon_b
    }
}

/// Create a symlink at `link` pointing to `target`.
#[cfg(unix)]
fn create_symlink(target: &Path, link: &Path, _executor: &dyn Executor) -> Result<()> {
    std::os::unix::fs::symlink(target, link).with_context(|| {
        format!(
            "creating symlink {} -> {}",
            link.display(),
            target.display()
        )
    })?;

    Ok(())
}

/// Create a symlink at `link` pointing to `target`.
#[cfg(windows)]
fn create_symlink(target: &Path, link: &Path, executor: &dyn Executor) -> Result<()> {
    let is_dir = target.is_dir();
    if is_dir {
        match std::os::windows::fs::symlink_dir(target, link) {
            Ok(()) => Ok(()),
            Err(e) => create_junction(target, link, executor)
                .with_context(|| format!("directory symlink failed: {e}")),
        }
    } else {
        std::os::windows::fs::symlink_file(target, link).map_err(anyhow::Error::from)
    }
    .with_context(|| {
        format!(
            "creating symlink {} -> {} (enable Developer Mode or run as administrator)",
            link.display(),
            target.display()
        )
    })?;

    Ok(())
}

/// Remove a symlink, handling platform differences.
///
/// On Windows, directory symlinks must be removed with `remove_dir` (not
/// `remove_file`). Rust's `symlink_metadata().is_dir()` returns `false` for
/// symlinks, so the raw `FILE_ATTRIBUTE_DIRECTORY` flag detects directory
/// symlinks. If `remove_dir` still fails with OS error 5 (access denied),
/// `remove_dir_all` retries through the standard library without invoking a
/// command shell.
fn remove_symlink(path: &Path, _executor: &dyn Executor) -> Result<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(e.into()),
        Err(e) => {
            return Err(
                anyhow::Error::new(e).context(format!("reading metadata: {}", path.display()))
            );
        }
    };
    if is_dir_like(&meta) {
        match std::fs::remove_dir(path) {
            Ok(()) => {}
            #[cfg(windows)]
            Err(e) if e.raw_os_error() == Some(5) => {
                std::fs::remove_dir_all(path)
                    .with_context(|| format!("removing directory symlink: {}", path.display()))?;
            }
            Err(e) => return Err(e.into()),
        }
    } else {
        std::fs::remove_file(path).with_context(|| format!("removing file: {}", path.display()))?;
    }
    Ok(())
}

/// Check if metadata represents a managed link-like entry.
#[cfg_attr(
    not(windows),
    allow(
        unused_variables,
        reason = "path is only needed to validate Windows reparse points"
    )
)]
fn is_link_like(path: &Path, meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0 && std::fs::read_link(path).is_ok()
    }
    #[cfg(not(windows))]
    {
        meta.is_symlink()
    }
}

/// Check if metadata represents a directory-like entry.
fn is_dir_like(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x10 != 0 // FILE_ATTRIBUTE_DIRECTORY
    }
    #[cfg(not(windows))]
    {
        meta.is_dir()
    }
}

/// Create a Windows directory junction as a fallback for directory symlinks.
#[cfg(windows)]
pub(super) fn create_junction(target: &Path, link: &Path, executor: &dyn Executor) -> Result<()> {
    let link_arg = link.to_string_lossy();
    let target_arg = target.to_string_lossy();
    let result = crate::runtime::exec::windows::CmdCommand::new("mklink")
        .arg("/J")
        .arg(link_arg.as_ref())
        .arg(target_arg.as_ref())
        .run_unchecked(executor)?;
    if result.success {
        Ok(())
    } else {
        Err(crate::engine::resource::ResourceError::command_failed(
            "mklink",
            format!(
                "create junction '{}' -> '{}': {}",
                link.display(),
                target.display(),
                command_output(&result)
            ),
        )
        .into())
    }
}

#[cfg(windows)]
fn command_output(result: &crate::runtime::exec::ExecResult) -> String {
    let stdout = result.stdout.trim();
    let stderr = result.stderr.trim();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => "no output".to_string(),
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (false, false) => format!("{stdout}; {stderr}"),
    }
}
