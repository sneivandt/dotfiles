use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

use super::platform::{is_link_like, remove_symlink};
use crate::runtime::exec::Executor;

/// Copy `source` into `target`, replacing the symlink that currently lives at
/// `target`. Files are staged to a sibling temp path first so that the window
/// where `target` is absent is as small as possible. Directories are handled
/// recursively via [`crate::runtime::fs::copy_dir_recursive`]; symlinks within
/// the source tree are recreated as symlinks rather than followed, preventing
/// unintended traversal outside the source tree.
pub(super) fn copy_into_place(source: &Path, target: &Path, executor: &dyn Executor) -> Result<()> {
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
            return Err(anyhow::anyhow!(
                "refusing to overwrite non-symlink target: {}",
                target.display()
            ));
        }
        None => {}
    }

    std::fs::rename(&tmp, target)
        .with_context(|| format!("rename {} to {}", tmp.display(), target.display()))?;

    guard.persist();
    Ok(())
}

/// Copy a directory: stage into a sibling temp directory, remove the
/// symlink/junction, then rename the temp directory into place. Falls back to a
/// plain copy+delete when the rename crosses a filesystem boundary (EXDEV).
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
            return Err(anyhow::anyhow!(
                "refusing to overwrite non-symlink target: {}",
                target.display()
            ));
        }
        None => {}
    }

    match std::fs::rename(&tmp, target) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            crate::runtime::fs::copy_dir_recursive(&tmp, target, false).with_context(|| {
                format!("cross-fs copy {} to {}", tmp.display(), target.display())
            })?;
            guard.persist();
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
