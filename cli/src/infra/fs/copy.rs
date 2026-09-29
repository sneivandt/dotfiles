use anyhow::{Context as _, Result};
use std::path::Path;

/// Recursively copy a directory tree, preserving Unix directory permissions.
///
/// When `skip_git` is `true`, `.git` directories are skipped — useful when
/// copying from a cloned repository where Git metadata is unwanted.
///
/// Symlinks and Windows junctions within the source tree are **not followed**:
/// each link is recreated as a symlink in `dst` pointing to the same target.
/// On Windows this requires Developer Mode or elevated privileges.
/// Failure to recreate a link fails the copy rather than silently omitting it.
/// This prevents unexpected traversal of symlinks that point outside the
/// intended source tree. Existing destination links are rejected rather than
/// followed, so copying into an existing tree cannot write through those links.
///
/// # Errors
///
/// Returns an error if the destination directory cannot be created, a source
/// entry cannot be read, a file cannot be copied, a symlink cannot be recreated,
/// or (on Unix) directory permissions cannot be preserved.
pub fn copy_dir_recursive(src: &Path, dst: &Path, skip_git: bool) -> Result<()> {
    reject_destination_link(dst)?;
    std::fs::create_dir_all(dst)
        .with_context(|| format!("creating directory {}", dst.display()))?;
    #[cfg(unix)]
    let permissions = std::fs::metadata(src)
        .with_context(|| format!("reading directory metadata for {}", src.display()))?
        .permissions();
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
            if skip_git && entry.file_name() == ".git" {
                continue;
            }
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
