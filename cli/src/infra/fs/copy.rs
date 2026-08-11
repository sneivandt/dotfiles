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
            let link_target = std::fs::read_link(&src_path)
                .with_context(|| format!("reading symlink {}", src_path.display()))?;
            let result =
                super::create_native_symlink(&link_target, &dst_path, super::is_dir_like(&meta));
            #[cfg(unix)]
            result.with_context(|| {
                format!(
                    "creating symlink {} -> {}",
                    dst_path.display(),
                    link_target.display()
                )
            })?;
            #[cfg(windows)]
            {
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
                let _ = result;
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
