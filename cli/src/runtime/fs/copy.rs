use anyhow::{Context as _, Result};
use std::path::Path;

/// Recursively copy a directory tree.
///
/// When `skip_git` is `true`, `.git` directories are skipped — useful when
/// copying from a cloned repository where Git metadata is unwanted.
///
/// Symlinks within the source tree are **not followed**: each symlink is
/// recreated in `dst` pointing to the same link target.  On Unix this always
/// succeeds; on Windows it requires Developer Mode or elevated privileges and
/// logs a warning (rather than failing) when the privilege check is not met.
/// This prevents unexpected traversal of symlinks that point outside the
/// intended source tree.
///
/// # Errors
///
/// Returns an error if the destination directory cannot be created, a source
/// entry cannot be read, a file cannot be copied, or (on Unix) a symlink
/// cannot be recreated.
pub fn copy_dir_recursive(src: &Path, dst: &Path, skip_git: bool) -> Result<()> {
    copy_dir_recursive_inner(src, dst, skip_git)
}

fn copy_dir_recursive_inner(src: &Path, dst: &Path, skip_git: bool) -> Result<()> {
    std::fs::create_dir_all(dst)
        .with_context(|| format!("creating directory {}", dst.display()))?;
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

        if meta.file_type().is_symlink() {
            // Recreate the symlink in dst rather than following it, preventing
            // traversal of symlinks that point outside the intended source tree.
            #[cfg(unix)]
            {
                let link_target = std::fs::read_link(&src_path)
                    .with_context(|| format!("reading symlink {}", src_path.display()))?;
                std::os::unix::fs::symlink(&link_target, &dst_path).with_context(|| {
                    format!(
                        "creating symlink {} -> {}",
                        dst_path.display(),
                        link_target.display()
                    )
                })?;
            }
            #[cfg(windows)]
            {
                // On Windows, attempt to recreate the symlink.  This requires
                // either Developer Mode or elevated privileges; if it fails we
                // log a warning and continue rather than silently dropping the
                // entry.
                //
                // Use the symlink's own metadata (`meta`, from
                // `symlink_metadata()`) to decide whether this is a directory
                // or file symlink.  On Windows, directory symlinks carry
                // FILE_ATTRIBUTE_DIRECTORY on the reparse point itself, so
                // `meta.is_dir()` is reliable even for dangling symlinks and
                // symlinks with relative targets that do not exist in the
                // current working directory.  This avoids the previous
                // approach of calling `link_target.is_dir()` or
                // `src_path.is_dir()`, both of which follow the link and
                // return `false` when the target is absent or relative.
                let link_target = std::fs::read_link(&src_path)
                    .with_context(|| format!("reading symlink {}", src_path.display()))?;
                let result = if meta.is_dir() {
                    std::os::windows::fs::symlink_dir(&link_target, &dst_path)
                } else {
                    std::os::windows::fs::symlink_file(&link_target, &dst_path)
                };
                if let Err(e) = result {
                    tracing::warn!(
                        "skipping symlink {} -> {}: {e} (enable Developer Mode or run as administrator)",
                        dst_path.display(),
                        link_target.display(),
                    );
                }
            }
            #[cfg(not(any(unix, windows)))]
            {
                tracing::warn!(
                    "skipping symlink entry {} while copying to {}: symlink creation is unsupported on this platform",
                    src_path.display(),
                    dst_path.display()
                );
            }
        } else if meta.is_dir() {
            if skip_git && entry.file_name() == ".git" {
                continue;
            }
            copy_dir_recursive_inner(&src_path, &dst_path, skip_git)?;
        } else {
            std::fs::copy(&src_path, &dst_path).with_context(|| {
                format!("copying {} to {}", src_path.display(), dst_path.display())
            })?;
        }
    }
    Ok(())
}
