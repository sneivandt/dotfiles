//! File-system helpers and the injectable [`FileSystemOps`] trait.
//!
//! This module is the single source of truth for filesystem operations:
//!
//! - [`FileSystemOps`] / [`SystemFileSystemOps`] — injectable trait for
//!   tasks that need to be unit-tested without touching the real filesystem.
//! - [`ensure_parent_dir`] / [`remove_existing`] / [`copy_dir_recursive`] —
//!   shared helper functions for resource `apply()` methods.
use anyhow::{Context as _, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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

/// Canonicalize a path, stripping the Windows extended-length `\\?\` prefix.
///
/// On Windows, [`std::fs::canonicalize`] returns paths prefixed with `\\?\`,
/// which breaks many tools (e.g. `Invoke-WebRequest -OutFile`). This helper
/// strips that prefix so paths remain compatible with typical Windows
/// tooling.
///
/// # Errors
///
/// Returns an error if the path does not exist or cannot be resolved.
pub fn canonicalize(path: &Path) -> Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)
        .with_context(|| format!("canonicalizing {}", path.display()))?;

    #[cfg(windows)]
    {
        let s = canonical.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return Ok(PathBuf::from(stripped));
        }
    }

    Ok(canonical)
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
    if let Ok(metadata) = path.symlink_metadata() {
        if metadata.is_dir() {
            std::fs::remove_dir(path)
                .with_context(|| format!("remove existing: {}", path.display()))?;
        } else {
            std::fs::remove_file(path)
                .with_context(|| format!("remove existing: {}", path.display()))?;
        }
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
/// link itself.  Circular symlinks are detected via canonical-path tracking
/// and reported as an error.
///
/// # Errors
///
/// Returns an error if the destination directory cannot be created, a source
/// entry cannot be read, a file cannot be copied, or a symlink cycle is
/// detected.
pub fn copy_dir_recursive(src: &Path, dst: &Path, skip_git: bool) -> Result<()> {
    let canonical = src
        .canonicalize()
        .with_context(|| format!("canonicalizing {}", src.display()))?;
    let mut visited = HashSet::new();
    visited.insert(canonical);
    copy_dir_recursive_inner(src, dst, skip_git, &mut visited)
}

fn copy_dir_recursive_inner(
    src: &Path,
    dst: &Path,
    skip_git: bool,
    visited: &mut HashSet<PathBuf>,
) -> Result<()> {
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
            let canonical = src_path
                .canonicalize()
                .with_context(|| format!("canonicalizing {}", src_path.display()))?;
            if !visited.insert(canonical) {
                anyhow::bail!("symlink cycle detected at {}", src_path.display());
            }
            copy_dir_recursive_inner(&src_path, &dst_path, skip_git, visited)?;
        } else {
            std::fs::copy(&src_path, &dst_path).with_context(|| {
                format!("copying {} to {}", src_path.display(), dst_path.display())
            })?;
        }
    }
    Ok(())
}

/// RAII guard that removes a temporary file when dropped.
///
/// Use this instead of manual cleanup closures when staging content through a
/// temp file.  Call [`persist`](Self::persist) to prevent deletion (e.g., after
/// a successful rename).
///
/// # Examples
///
/// ```ignore
/// let tmp = TempPath::new(dir.join(".update.tmp"));
/// std::fs::write(tmp.path(), data)?;
/// std::fs::rename(tmp.path(), final_path)?;
/// tmp.persist(); // prevent cleanup since rename succeeded
/// ```
#[derive(Debug)]
pub struct TempPath {
    path: PathBuf,
    active: bool,
}

impl TempPath {
    /// Create a guard for the given temporary file path.
    #[must_use]
    pub const fn new(path: PathBuf) -> Self {
        Self { path, active: true }
    }

    /// Borrow the underlying path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Disarm the guard so the file is **not** removed on drop.
    pub const fn persist(&mut self) {
        self.active = false;
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        if self.active {
            std::fs::remove_file(&self.path).ok();
        }
    }
}

/// RAII guard that recursively removes a temporary directory when dropped.
///
/// Analogous to [`TempPath`] but for directory trees.  Call
/// [`persist`](Self::persist) to prevent deletion.
#[derive(Debug)]
pub struct TempDir {
    path: PathBuf,
    active: bool,
}

impl TempDir {
    /// Create a guard for the given temporary directory path.
    #[must_use]
    pub const fn new(path: PathBuf) -> Self {
        Self { path, active: true }
    }

    /// Borrow the underlying path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Disarm the guard so the directory is **not** removed on drop.
    pub const fn persist(&mut self) {
        self.active = false;
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if self.active {
            std::fs::remove_dir_all(&self.path).ok();
        }
    }
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

    #[cfg(unix)]
    #[test]
    fn detects_symlink_cycle() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("file.txt"), b"content").unwrap();
        std::fs::create_dir(src.path().join("sub")).unwrap();
        std::os::unix::fs::symlink(src.path(), src.path().join("sub/loop")).unwrap();

        let dst = tempfile::tempdir().unwrap();
        let target = dst.path().join("out");
        let err = copy_dir_recursive(src.path(), &target, false).unwrap_err();
        assert!(
            format!("{err:?}").contains("symlink cycle"),
            "expected symlink cycle error, got: {err:?}"
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
    fn remove_existing_removes_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let target_dir = dir.path().join("target");
        std::fs::create_dir(&target_dir).unwrap();
        remove_existing(&target_dir).unwrap();
        assert!(!target_dir.exists());
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

    // -----------------------------------------------------------------------
    // TempPath
    // -----------------------------------------------------------------------

    #[test]
    fn temp_path_removes_file_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tmp_file");
        std::fs::write(&file, "data").unwrap();
        assert!(file.exists());

        {
            let _guard = TempPath::new(file.clone());
        }
        assert!(!file.exists(), "file should be removed on drop");
    }

    #[test]
    fn temp_path_persist_prevents_removal() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("keep_file");
        std::fs::write(&file, "data").unwrap();

        {
            let mut guard = TempPath::new(file.clone());
            guard.persist();
        }
        assert!(file.exists(), "file should remain after persist + drop");
    }

    #[test]
    fn temp_path_noop_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("nonexistent");
        // Should not panic when the file doesn't exist
        let _guard = TempPath::new(file);
    }

    // -----------------------------------------------------------------------
    // TempDir
    // -----------------------------------------------------------------------

    #[test]
    fn temp_dir_removes_directory_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let td = dir.path().join("tmp_dir");
        std::fs::create_dir(&td).unwrap();
        std::fs::write(td.join("child.txt"), "data").unwrap();
        assert!(td.exists());

        {
            let _guard = TempDir::new(td.clone());
        }
        assert!(!td.exists(), "directory should be removed on drop");
    }

    #[test]
    fn temp_dir_persist_prevents_removal() {
        let dir = tempfile::tempdir().unwrap();
        let td = dir.path().join("keep_dir");
        std::fs::create_dir(&td).unwrap();

        {
            let mut guard = TempDir::new(td.clone());
            guard.persist();
        }
        assert!(td.exists(), "directory should remain after persist + drop");
    }

    #[test]
    fn example_failing_test() {
        assert_eq!(1, 2, "this test is intentionally failing to trigger the CI fix agent");
    }
}
