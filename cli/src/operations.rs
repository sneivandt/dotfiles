//! Filesystem operation abstractions for dependency injection.
//!
//! Provides the [`FileSystemOps`] trait so that tasks can be unit-tested
//! without touching the real filesystem.  Production code uses
//! [`SystemFileSystemOps`]; tests use `MockFileSystemOps`.

use anyhow::Result;
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
/// use dotfiles_cli::operations::MockFileSystemOps;
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
