//! File-system resource helpers and the injectable [`FileSystemOps`] trait.
//!
//! This module is the single source of truth for filesystem operations:
//!
//! - [`FileSystemOps`] / [`SystemFileSystemOps`] — injectable trait for
//!   tasks that need to be unit-tested without touching the real filesystem.
//! - [`ensure_parent_dir`] / [`remove_existing`] / [`copy_dir_recursive`] —
//!   shared helper functions for resource `apply()` methods.
use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

/// Abstraction over filesystem queries used by tasks.
///
/// Implement this trait to swap in a mock during unit tests, keeping task
/// logic independent of real I/O.  The production implementation is
/// [`SystemFileSystemOps`].
pub trait FileSystemOps: Send + Sync + std::fmt::Debug {
    /// Returns `true` if `path` exists on the filesystem.
    fn exists(&self, path: &Path) -> bool;

    /// Returns `true` if `path` is a regular file (not a directory or broken symlink).
    fn is_file(&self, path: &Path) -> bool;

    /// Returns the immediate child paths inside `path`.
    ///
    /// # Errors
    ///
    /// Returns an error if `path` cannot be opened or read as a directory.
    fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>>;

    /// Read the target of the symbolic link at `path`.
    ///
    /// # Errors
    ///
    /// Returns an error if `path` is not a symlink or cannot be read.
    fn read_link(&self, path: &Path) -> std::io::Result<PathBuf>;

    /// Remove the file or empty directory at `path`.
    ///
    /// Calls `std::fs::remove_file` for files/symlinks and
    /// `std::fs::remove_dir` for directories.
    ///
    /// # Errors
    ///
    /// Returns an error if removal fails.
    fn remove(&self, path: &Path) -> std::io::Result<()>;
}

/// Production [`FileSystemOps`] implementation that delegates to [`std::fs`].
#[derive(Debug, Default)]
pub struct SystemFileSystemOps;

impl FileSystemOps for SystemFileSystemOps {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>> {
        std::fs::read_dir(path)?
            .map(|e| e.map(|entry| entry.path()).map_err(Into::into))
            .collect()
    }

    fn read_link(&self, path: &Path) -> std::io::Result<PathBuf> {
        std::fs::read_link(path)
    }

    fn remove(&self, path: &Path) -> std::io::Result<()> {
        let meta = std::fs::symlink_metadata(path)?;
        if meta.is_dir() {
            std::fs::remove_dir(path)
        } else {
            std::fs::remove_file(path)
        }
    }
}

/// Mock [`FileSystemOps`] for unit tests.
///
/// Pre-configure existing paths, regular files, and directory listings using
/// the builder-style methods, then pass `Arc::new(mock)` to task constructors
/// that accept a [`FileSystemOps`] (e.g., `InstallGitHooks::with_fs_ops`).
///
/// # Example
///
/// ```ignore
/// use dotfiles_cli::resources::helpers::fs::MockFileSystemOps;
/// use std::path::PathBuf;
///
/// let fs = MockFileSystemOps::new()
///     .with_file("/repo/hooks/pre-commit")
///     .with_dir_entries(
///         "/repo/hooks",
///         vec![PathBuf::from("/repo/hooks/pre-commit")],
///     );
/// ```
#[cfg(test)]
#[derive(Debug, Default)]
pub struct MockFileSystemOps {
    existing: Vec<PathBuf>,
    files: Vec<PathBuf>,
    dirs: std::collections::HashMap<PathBuf, Vec<PathBuf>>,
    symlinks: std::collections::HashMap<PathBuf, PathBuf>,
    removed: std::sync::Mutex<std::collections::HashSet<PathBuf>>,
}

#[cfg(test)]
impl MockFileSystemOps {
    /// Create an empty mock with nothing configured.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark `path` as existing without making it a file or directory.
    #[must_use]
    pub fn with_existing(mut self, path: impl Into<PathBuf>) -> Self {
        let p = path.into();
        if !self.existing.contains(&p) {
            self.existing.push(p);
        }
        self
    }

    /// Mark `path` as a regular file (also marks it as existing).
    #[must_use]
    pub fn with_file(mut self, path: impl Into<PathBuf>) -> Self {
        let p = path.into();
        if !self.existing.contains(&p) {
            self.existing.push(p.clone());
        }
        if !self.files.contains(&p) {
            self.files.push(p);
        }
        self
    }

    /// Set the directory entries returned by [`FileSystemOps::read_dir`] for `dir`.
    ///
    /// Also marks `dir` itself as existing.
    #[must_use]
    pub fn with_dir_entries(mut self, dir: impl Into<PathBuf>, entries: Vec<PathBuf>) -> Self {
        let d = dir.into();
        if !self.existing.contains(&d) {
            self.existing.push(d.clone());
        }
        self.dirs.insert(d, entries);
        self
    }

    /// Register `path` as a symbolic link pointing to `target`.
    ///
    /// The path is also implicitly marked as existing.
    #[must_use]
    pub fn with_symlink(mut self, path: impl Into<PathBuf>, target: impl Into<PathBuf>) -> Self {
        let p = path.into();
        let t = target.into();
        if !self.existing.contains(&p) {
            self.existing.push(p.clone());
        }
        self.symlinks.insert(p, t);
        self
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
impl FileSystemOps for MockFileSystemOps {
    fn exists(&self, path: &Path) -> bool {
        !self
            .removed
            .lock()
            .expect("mock removed set poisoned")
            .contains(path)
            && self.existing.iter().any(|p| p == path)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.iter().any(|p| p == path)
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>> {
        self.dirs
            .get(path)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("mock: no entries configured for {}", path.display()))
    }

    fn read_link(&self, path: &Path) -> std::io::Result<PathBuf> {
        if self
            .removed
            .lock()
            .expect("mock removed set poisoned")
            .contains(path)
        {
            return Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        }
        self.symlinks
            .get(path)
            .cloned()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))
    }

    fn remove(&self, path: &Path) -> std::io::Result<()> {
        self.removed
            .lock()
            .expect("mock removed set poisoned")
            .insert(path.to_path_buf());
        Ok(())
    }
}

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
