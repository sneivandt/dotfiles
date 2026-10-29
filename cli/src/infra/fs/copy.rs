use anyhow::{Context as _, Result};
use std::path::Path;

/// Recursively copy a directory tree, preserving Unix directory permissions.
///
/// When `skip_git` is `true`, `.git` entries are skipped, including worktree
/// and submodule gitfiles — useful when Git metadata is unwanted.
///
/// Symlinks and Windows junctions within the source tree are **not followed**:
/// each link is recreated as a symlink in `dst` pointing to the same target.
/// On Windows this requires Developer Mode or elevated privileges.
/// Failure to recreate a link fails the copy rather than silently omitting it.
/// This prevents unexpected traversal of symlinks that point outside the
/// intended source tree. Existing destination links are rejected rather than
/// followed, so copying into an existing tree cannot write through those links.
/// A destination inside the source tree is rejected before anything is changed,
/// including when an existing ancestor aliases the source through a link.
///
/// # Errors
///
/// Returns an error if the destination directory cannot be created, a source
/// entry cannot be read, a file cannot be copied, a symlink cannot be recreated,
/// or (on Unix) directory permissions cannot be preserved.
pub fn copy_dir_recursive(src: &Path, dst: &Path, skip_git: bool) -> Result<()> {
    let source_metadata = std::fs::metadata(src)
        .with_context(|| format!("reading directory metadata for {}", src.display()))?;
    anyhow::ensure!(
        source_metadata.is_dir(),
        "copy source is not a directory: {}",
        src.display()
    );
    reject_destination_link(dst)?;
    reject_recursive_destination(src, dst)?;
    std::fs::create_dir_all(dst)
        .with_context(|| format!("creating directory {}", dst.display()))?;
    #[cfg(unix)]
    let permissions = source_metadata.permissions();
    // Keep staged contents private while copying, and defer the source mode
    // until children are populated so read-only directories can be copied.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dst, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("restricting directory {}", dst.display()))?;
    }
    for entry in
        std::fs::read_dir(src).with_context(|| format!("reading directory {}", src.display()))?
    {
        let entry = entry.with_context(|| format!("reading entry in {}", src.display()))?;
        if skip_git && entry.file_name() == ".git" {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        // Use symlink_metadata() so we detect symlinks without following them.
        let meta = src_path
            .symlink_metadata()
            .with_context(|| format!("reading metadata for {}", src_path.display()))?;

        if is_link_like(&src_path, &meta) {
            // Recreate the symlink in dst rather than following it, preventing
            // traversal of symlinks that point outside the intended source tree.
            let link_target = std::fs::read_link(&src_path)
                .with_context(|| format!("reading symlink {}", src_path.display()))?;
            super::create_native_symlink(&link_target, &dst_path, super::is_dir_like(&meta))
                .with_context(|| {
                    format!(
                        "creating symlink {} -> {}",
                        dst_path.display(),
                        link_target.display()
                    )
                })?;
        } else if meta.is_dir() {
            copy_dir_recursive(&src_path, &dst_path, skip_git)?;
        } else {
            reject_destination_link(&dst_path)?;
            std::fs::copy(&src_path, &dst_path).with_context(|| {
                format!("copying {} to {}", src_path.display(), dst_path.display())
            })?;
        }
    }
    #[cfg(unix)]
    std::fs::set_permissions(dst, permissions)
        .with_context(|| format!("preserving directory permissions for {}", dst.display()))?;
    Ok(())
}

fn reject_recursive_destination(src: &Path, dst: &Path) -> Result<()> {
    let source = std::fs::canonicalize(src)
        .with_context(|| format!("resolving copy source {}", src.display()))?;
    let destination = std::path::absolute(dst)
        .with_context(|| format!("resolving copy destination {}", dst.display()))?;

    // Missing destination components have no canonical path yet. Resolve the
    // nearest existing ancestor first so symlinks followed by `..` retain their
    // filesystem meaning instead of being normalized lexically too early.
    for ancestor in destination.ancestors() {
        match std::fs::canonicalize(ancestor) {
            Ok(mut resolved) => {
                for component in destination.strip_prefix(ancestor)?.components() {
                    if component == std::path::Component::ParentDir {
                        resolved.pop();
                    } else {
                        resolved.push(component.as_os_str());
                    }
                }
                anyhow::ensure!(
                    !resolved.starts_with(&source),
                    "refusing to copy {} into itself: {}",
                    src.display(),
                    dst.display()
                );
                return Ok(());
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && ancestor.parent().is_some() => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("resolving copy destination {}", dst.display()));
            }
        }
    }
    anyhow::bail!("cannot resolve copy destination {}", dst.display())
}

/// Reject an existing symlink or junction before copying into `path`.
///
/// # Errors
///
/// Returns an error if metadata cannot be read or the destination is a link.
pub fn reject_destination_link(path: &Path) -> Result<()> {
    if let Some(metadata) = super::symlink_metadata_optional(path, "reading copy destination")? {
        anyhow::ensure!(
            !is_link_like(path, &metadata),
            "refusing to copy through destination link {}",
            path.display()
        );
    }
    Ok(())
}

fn is_link_like(path: &Path, metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        metadata.is_symlink()
            || (metadata.file_attributes() & 0x400 != 0 && std::fs::read_link(path).is_ok())
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        metadata.is_symlink()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_resolution_preserves_non_missing_io_errors() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        std::fs::create_dir(&source).unwrap();
        let destination = root.path().join("invalid\0destination");
        let expected = std::fs::canonicalize(&destination).unwrap_err().kind();
        assert_ne!(expected, std::io::ErrorKind::NotFound);

        let error = reject_recursive_destination(&source, &destination).unwrap_err();

        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            expected
        );
        assert_eq!(
            error.to_string(),
            format!("resolving copy destination {}", destination.display())
        );
    }
}
