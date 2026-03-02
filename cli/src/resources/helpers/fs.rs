//! File-system resource helpers.
use anyhow::{Context as _, Result};
use std::path::Path;

/// Ensure the parent directory of `path` exists, creating it (and any
/// ancestors) if necessary.
///
/// This is a shared helper for resource `apply()` methods that need to
/// create parent directories before writing a file or symlink.
///
/// # Errors
///
/// Returns an error if the directory cannot be created.
pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create parent: {}", parent.display()))?;
    }
    Ok(())
}

/// Remove an existing file or symlink at `path`, including broken symlinks.
///
/// This is a shared helper for resource `apply()` methods that need to
/// replace an existing target.  Does nothing if `path` does not exist.
///
/// # Errors
///
/// Returns an error if the path exists but cannot be removed.
pub fn remove_existing(path: &Path) -> Result<()> {
    if path.exists() || path.symlink_metadata().is_ok() {
        std::fs::remove_file(path)
            .with_context(|| format!("remove existing: {}", path.display()))?;
    }
    Ok(())
}

/// Recursively copy a directory tree.
///
/// When `skip_git` is `true`, `.git` directories are skipped — useful when
/// copying from a cloned repository where Git metadata is unwanted.
///
/// Symlinks within the source tree are *followed*: the function uses
/// [`Path::is_dir`] (which follows symlinks) so directory symlinks are
/// recursed into and their contents materialised rather than copying the
/// link itself.
///
/// # Errors
///
/// Returns an error if the destination directory cannot be created, a source
/// entry cannot be read, or a file cannot be copied.
pub fn copy_dir_recursive(src: &Path, dst: &Path, skip_git: bool) -> Result<()> {
    std::fs::create_dir_all(dst)
        .with_context(|| format!("creating directory {}", dst.display()))?;
    for entry in
        std::fs::read_dir(src).with_context(|| format!("reading directory {}", src.display()))?
    {
        let entry = entry.with_context(|| format!("reading entry in {}", src.display()))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            if skip_git && entry.file_name() == ".git" {
                continue;
            }
            copy_dir_recursive(&src_path, &dst_path, skip_git)?;
        } else {
            std::fs::copy(&src_path, &dst_path).with_context(|| {
                format!("copying {} to {}", src_path.display(), dst_path.display())
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn copies_files_and_subdirectories() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();

        std::fs::write(src.path().join("a.txt"), b"aaa").unwrap();
        std::fs::create_dir(src.path().join("sub")).unwrap();
        std::fs::write(src.path().join("sub/b.txt"), b"bbb").unwrap();

        let target = dst.path().join("out");
        copy_dir_recursive(src.path(), &target, false).unwrap();

        assert_eq!(std::fs::read(target.join("a.txt")).unwrap(), b"aaa");
        assert_eq!(std::fs::read(target.join("sub/b.txt")).unwrap(), b"bbb");
    }

    #[test]
    fn skips_git_directory_when_flag_set() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();

        std::fs::write(src.path().join("file.txt"), b"content").unwrap();
        std::fs::create_dir(src.path().join(".git")).unwrap();
        std::fs::write(src.path().join(".git/HEAD"), b"ref: refs/heads/main").unwrap();

        let target = dst.path().join("out");
        copy_dir_recursive(src.path(), &target, true).unwrap();

        assert!(target.join("file.txt").exists());
        assert!(
            !target.join(".git").exists(),
            ".git directory should be skipped"
        );
    }

    #[test]
    fn copies_git_directory_when_flag_not_set() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();

        std::fs::write(src.path().join("file.txt"), b"content").unwrap();
        std::fs::create_dir(src.path().join(".git")).unwrap();
        std::fs::write(src.path().join(".git/HEAD"), b"ref: refs/heads/main").unwrap();

        let target = dst.path().join("out");
        copy_dir_recursive(src.path(), &target, false).unwrap();

        assert!(target.join("file.txt").exists());
        assert!(
            target.join(".git/HEAD").exists(),
            ".git directory should be copied"
        );
    }

    // -----------------------------------------------------------------------
    // ensure_parent_dir
    // -----------------------------------------------------------------------

    #[test]
    fn ensure_parent_dir_creates_missing_parents() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b").join("file.txt");
        ensure_parent_dir(&nested).unwrap();
        assert!(dir.path().join("a").join("b").exists());
    }

    #[test]
    fn ensure_parent_dir_noop_when_parent_exists() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        ensure_parent_dir(&file).unwrap();
        assert!(dir.path().exists());
    }

    // -----------------------------------------------------------------------
    // remove_existing
    // -----------------------------------------------------------------------

    #[test]
    fn remove_existing_removes_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("target");
        std::fs::write(&file, "content").unwrap();
        remove_existing(&file).unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn remove_existing_noop_when_path_absent() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("nonexistent");
        remove_existing(&file).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn remove_existing_removes_broken_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink("/nonexistent/target", &link).unwrap();
        assert!(link.symlink_metadata().is_ok());
        remove_existing(&link).unwrap();
        assert!(link.symlink_metadata().is_err());
    }
}
