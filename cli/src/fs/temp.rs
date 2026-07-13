use std::path::{Path, PathBuf};

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
            match std::fs::remove_file(&self.path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    tracing::debug!(
                        "failed to remove temporary file {}: {error}",
                        self.path.display()
                    );
                }
            }
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
            match std::fs::remove_dir_all(&self.path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    tracing::debug!(
                        "failed to remove temporary directory {}: {error}",
                        self.path.display()
                    );
                }
            }
        }
    }
}
