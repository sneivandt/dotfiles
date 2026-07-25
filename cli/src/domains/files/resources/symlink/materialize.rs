use anyhow::{Context as _, Result};
use std::path::Path;

use super::platform::{is_link_like, remove_symlink};
use crate::infra::exec::Executor;
use crate::infra::fs::{rename_into_place, sibling_temp_path};

/// Copy `source` into `target`, replacing the symlink that currently lives at
/// `target`. Files are staged to a sibling temp path first so that the window
/// where `target` is absent is as small as possible. Directories are handled
/// recursively via [`crate::infra::fs::copy_dir_recursive`]; symlinks within
/// the source tree are recreated as symlinks rather than followed, preventing
/// unintended traversal outside the source tree.
pub(super) fn copy_into_place(source: &Path, target: &Path, executor: &dyn Executor) -> Result<()> {
    if source.is_dir() {
        copy_dir_into_place(source, target, executor)
    } else {
        copy_file_into_place(source, target, executor)
    }
}

/// Clear the link that currently occupies `target` so staged content can be
/// renamed over it.
///
/// Replacement is only ever performed on link-like targets: anything else is a
/// user file that this resource must not silently discard. `label` names the
/// link flavour for the error context (`"symlink"` on files, `"symlink/junction"`
/// for directories on Windows).
fn clear_link_target(target: &Path, executor: &dyn Executor, label: &str) -> Result<()> {
    match crate::infra::fs::symlink_metadata_optional(target, "stat target")? {
        Some(meta) if is_link_like(target, &meta) => remove_symlink(target, executor)
            .with_context(|| format!("remove {label}: {}", target.display())),
        Some(_) => Err(anyhow::anyhow!(
            "refusing to overwrite non-symlink target: {}",
            target.display()
        )),
        None => Ok(()),
    }
}

/// Copy a regular file: stage to a temp sibling, remove the symlink, rename
/// the temp file into place.
fn copy_file_into_place(source: &Path, target: &Path, executor: &dyn Executor) -> Result<()> {
    let tmp = sibling_temp_path(target, ".dotfiles_tmp");
    crate::infra::fs::copy_file(source, &tmp)?;

    let mut guard = crate::infra::fs::TempPath::new(tmp.clone());

    clear_link_target(target, executor, "symlink")?;

    rename_into_place(&tmp, target)?;

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
    let mut guard = crate::infra::fs::TempDir::new(tmp.clone());

    crate::infra::fs::copy_dir_recursive(source, &tmp, false)
        .with_context(|| format!("recursive copy {} to {}", source.display(), tmp.display()))?;

    clear_link_target(target, executor, "symlink/junction")?;

    match std::fs::rename(&tmp, target) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            crate::infra::fs::copy_dir_recursive(&tmp, target, false).with_context(|| {
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
    let Some(meta) = crate::infra::fs::symlink_metadata_optional(tmp, "stat temp path")? else {
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
