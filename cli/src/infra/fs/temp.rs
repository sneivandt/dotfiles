use std::path::{Path, PathBuf};

/// RAII guard that removes a temporary path when dropped.
///
/// Use this instead of manual cleanup closures when staging content through a
/// temporary file or directory.  Call [`persist`](Self::persist) to prevent
/// deletion (e.g., after a successful rename).
///
/// Construct with [`file`](Self::file) for a single file or
/// [`dir`](Self::dir) for a directory tree; the two differ only in how the
/// path is removed.
///
/// # Examples
///
/// ```ignore
/// let mut tmp = TempGuard::file(dir.join(".update.tmp"));
/// std::fs::write(tmp.path(), data)?;
/// std::fs::rename(tmp.path(), final_path)?;
/// tmp.persist(); // prevent cleanup since rename succeeded
/// ```
#[derive(Debug)]
pub struct TempGuard {
    path: PathBuf,
    active: bool,
    kind: TempKind,
}

/// What a [`TempGuard`] cleans up, and therefore how it removes it.
#[derive(Debug, Clone, Copy)]
enum TempKind {
    File,
    Dir,
}

impl TempKind {
    const fn label(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Dir => "directory",
        }
    }

    fn remove(self, path: &Path) -> std::io::Result<()> {
        match self {
            Self::File => std::fs::remove_file(path),
            Self::Dir => std::fs::remove_dir_all(path),
        }
    }
}

impl TempGuard {
    /// Create a guard that removes the given temporary *file* on drop.
    #[must_use]
    pub const fn file(path: PathBuf) -> Self {
        Self {
            path,
            active: true,
            kind: TempKind::File,
        }
    }

    /// Create a guard over a *file* in `dir` whose name cannot collide with a
    /// concurrent call.
    ///
    /// The name is `{prefix}-{pid}-{seq}-{suffix}`. The PID alone is not
    /// enough: several threads of one process stage content under the same
    /// logical name, so a shared path lets one call's guard delete a file
    /// another call is still writing. The counter closes that window; the PID
    /// closes the equivalent one across processes.
    ///
    /// Note this only reserves a *name* — the caller still creates the file.
    #[must_use]
    pub fn unique_file(dir: &Path, prefix: &str, suffix: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let pid = std::process::id();
        Self::file(dir.join(format!("{prefix}-{pid}-{seq}-{suffix}")))
    }

    /// Create a guard that recursively removes the given temporary
    /// *directory* on drop.
    #[must_use]
    pub const fn dir(path: PathBuf) -> Self {
        Self {
            path,
            active: true,
            kind: TempKind::Dir,
        }
    }

    /// Borrow the underlying path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Disarm the guard so the path is **not** removed on drop.
    pub const fn persist(&mut self) {
        self.active = false;
    }
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        match self.kind.remove(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::debug!(
                    "failed to remove temporary {} {}: {error}",
                    self.kind.label(),
                    self.path.display()
                );
            }
        }
    }
}
