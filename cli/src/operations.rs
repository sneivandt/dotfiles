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
}

/// Mock [`FileSystemOps`] for unit tests.
///
/// Pre-configure existing paths, regular files, and directory listings using
/// the builder-style methods, then pass `Arc::new(mock)` to
/// [`Context::with_fs_ops`](crate::tasks::Context::with_fs_ops).
///
/// # Example
///
/// ```
/// use dotfiles::operations::MockFileSystemOps;
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
}

#[cfg(test)]
impl FileSystemOps for MockFileSystemOps {
    fn exists(&self, path: &Path) -> bool {
        self.existing.iter().any(|p| p == path)
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
}
