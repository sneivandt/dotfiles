use anyhow::{Context as _, Result};
use std::path::Path;

/// Recursively copy a directory tree.
///
/// When `skip_git` is `true`, `.git` directories are skipped — useful when
/// copying from a cloned repository where Git metadata is unwanted.
///
/// Symlinks within the source tree are *followed*: the function uses
/// [`Path::is_dir`] (which follows symlinks) so directory symlinks are
/// recursed into and their contents materialised rather than copying the
/// link itself.
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
}
