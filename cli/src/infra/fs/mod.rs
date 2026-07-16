//! File-system helpers, temporary-path guards, and injectable operations.
//!
//! This module is the single source of truth for filesystem operations:
//!
//! - [`FileSystemOps`] / [`SystemFileSystemOps`] — injectable trait for
//!   tasks that need to be unit-tested without touching the real filesystem.
//! - [`ensure_parent_dir`] / [`remove_existing`] / [`copy_dir_recursive`] —
//!   shared helper functions for resource `apply()` methods.
use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

mod copy;
mod temp;

pub use copy::copy_dir_recursive;
pub use temp::{TempDir, TempPath};

/// Abstraction over filesystem queries used by tasks.
///
/// Implement this trait to swap in a mock during unit tests, keeping task
/// logic independent of real I/O.  The production implementation is
/// [`SystemFileSystemOps`].
#[cfg_attr(test, mockall::automock)]
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

/// Read symlink metadata and return `None` when the path is absent.
///
/// # Errors
///
/// Returns an error if metadata cannot be read for a reason other than the path
/// being absent.
pub fn symlink_metadata_optional(path: &Path, action: &str) -> Result<Option<std::fs::Metadata>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(anyhow::Error::new(error).context(format!("{action}: {}", path.display())))
        }
    }
}

/// Return a standard validation message when a required source path is missing.
#[must_use]
pub fn missing_source_reason(path: &Path) -> Option<String> {
    (!path.exists()).then(|| format!("source does not exist: {}", path.display()))
}

/// Prepare a target for replacement by creating its parent and removing any
/// existing file, symlink, or empty directory.
///
/// # Errors
///
/// Returns an error if the parent directory cannot be created or the existing
/// target cannot be removed.
pub fn prepare_target(path: &Path) -> Result<()> {
    ensure_parent_dir(path)?;
    remove_existing(path)
}

/// Read a file as bytes with consistent path context.
///
/// # Errors
///
/// Returns an error if the file cannot be read.
pub fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("read {}", path.display()))
}

/// Read a UTF-8 file with consistent path context.
///
/// # Errors
///
/// Returns an error if the file cannot be read as a string.
pub fn read_string(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))
}

/// Write file content with consistent path context.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
pub fn write(path: &Path, content: impl AsRef<[u8]>) -> Result<()> {
    std::fs::write(path, content).with_context(|| format!("write {}", path.display()))
}

/// Write file content after ensuring the parent directory exists.
///
/// # Errors
///
/// Returns an error if the parent directory cannot be created or the file
/// cannot be written.
pub fn write_with_parent(path: &Path, content: impl AsRef<[u8]>) -> Result<()> {
    ensure_parent_dir(path)?;
    write(path, content)
}

/// Copy a file with consistent path context.
///
/// # Errors
///
/// Returns an error if the source cannot be copied to the destination.
pub fn copy_file(source: &Path, target: &Path) -> Result<u64> {
    std::fs::copy(source, target)
        .with_context(|| format!("copy {} to {}", source.display(), target.display()))
}

/// Remove a file with consistent path context.
///
/// # Errors
///
/// Returns an error if the file cannot be removed.
pub fn remove_file(path: &Path) -> Result<()> {
    std::fs::remove_file(path).with_context(|| format!("remove {}", path.display()))
}

/// Remove a file if it exists and report whether anything was removed.
///
/// # Errors
///
/// Returns an error if metadata cannot be read or the existing file cannot be
/// removed.
pub fn remove_file_if_present(path: &Path, action: &str) -> Result<bool> {
    match symlink_metadata_optional(path, action)? {
        Some(_) => {
            remove_file(path)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Set a file executable by owner, group, and others.
///
/// # Errors
///
/// Returns an error if file metadata cannot be read or permissions cannot be
/// written.
#[cfg(unix)]
pub fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = std::fs::metadata(path)
        .with_context(|| format!("read executable metadata: {}", path.display()))?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
        .with_context(|| format!("set executable permissions: {}", path.display()))
}

/// Canonicalize a path, stripping the Windows extended-length `\\?\` prefix.
///
/// On Windows, [`std::fs::canonicalize`] returns paths prefixed with `\\?\`,
/// which breaks many tools (e.g. `Invoke-WebRequest -OutFile`). This helper
/// uses [`dunce::canonicalize`] to strip that prefix so paths remain
/// compatible with typical Windows tooling.
///
/// # Errors
///
/// Returns an error if the path does not exist or cannot be resolved.
pub fn canonicalize(path: &Path) -> Result<PathBuf> {
    dunce::canonicalize(path).with_context(|| format!("canonicalizing {}", path.display()))
}

/// Remove an existing file, symlink, or empty directory at `path`.
///
/// This is a shared helper for resource `apply()` methods that need to
/// replace an existing target.  Does nothing if `path` does not exist.
/// Non-empty directories still return an error instead of being removed
/// recursively.
///
/// # Errors
///
/// Returns an error if the path exists but cannot be removed.
pub fn remove_existing(path: &Path) -> Result<()> {
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("stat existing: {}", path.display()));
        }
    };

    if metadata.is_dir() {
        std::fs::remove_dir(path)
            .with_context(|| format!("remove existing: {}", path.display()))?;
    } else {
        std::fs::remove_file(path)
            .with_context(|| format!("remove existing: {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
