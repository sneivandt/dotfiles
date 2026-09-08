use anyhow::{Context as _, Result};
use std::path::Path;

use super::platform::{is_link_like, remove_symlink};
use crate::infra::exec::Executor;
use crate::infra::fs::{TempGuard, ensure_parent_dir, rename_into_place};

/// Copy `source` into `target`, replacing the symlink that currently lives at
/// `target`. Files are staged to a sibling temp path first so that the window
/// where `target` is absent is as small as possible. Directories are handled
/// recursively via [`crate::infra::fs::copy_dir_recursive`]; symlinks within
/// the source tree are recreated as symlinks rather than followed, preventing
/// unintended traversal outside the source tree.
pub(super) fn copy_into_place(source: &Path, target: &Path, executor: &dyn Executor) -> Result<()> {
    ensure_parent_dir(target)?;

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
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let (mut guard, mut staged_file) =
        TempGuard::create_unique_file(parent, ".dotfiles-materialize-file", "tmp")
            .with_context(|| format!("create temporary file beside {}", target.display()))?;
    let mut source_file = std::fs::File::open(source)
        .with_context(|| format!("open source file {}", source.display()))?;
    std::io::copy(&mut source_file, &mut staged_file)
        .with_context(|| format!("copy source file {}", source.display()))?;
    std::fs::set_permissions(guard.path(), source_file.metadata()?.permissions())?;
    drop(staged_file);

    clear_link_target(target, executor, "symlink")?;

    rename_into_place(guard.path(), target)?;

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
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let mut guard = TempGuard::create_unique_dir(parent, ".dotfiles-materialize-dir", "tmp")
        .with_context(|| format!("create temporary directory beside {}", target.display()))?;

    crate::infra::fs::copy_dir_recursive(source, guard.path(), false).with_context(|| {
        format!(
            "recursive copy {} to {}",
            source.display(),
            guard.path().display()
        )
    })?;

    clear_link_target(target, executor, "symlink/junction")?;

    match std::fs::rename(guard.path(), target) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            crate::infra::fs::copy_dir_recursive(guard.path(), target, false).with_context(
                || {
                    format!(
                        "cross-fs copy {} to {}",
                        guard.path().display(),
                        target.display()
                    )
                },
            )?;
        }
        Err(e) => {
            return Err(anyhow::Error::new(e).context(format!(
                "rename {} to {}",
                guard.path().display(),
                target.display()
            )));
        }
    }

    if !guard.path().exists() {
        guard.persist();
    }
    Ok(())
}
