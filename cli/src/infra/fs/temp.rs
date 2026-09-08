use std::path::{Path, PathBuf};
use std::{fs::File, io};

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

    fn remove(self, path: &Path) -> io::Result<()> {
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

    /// Create and guard a new file in `dir` whose name cannot collide with a
    /// concurrent call.
    ///
    /// The name is `{prefix}-{pid}-{seq}-{suffix}`. The PID alone is not
    /// enough: several threads of one process stage content under the same
    /// logical name, so a shared path lets one call's guard delete a file
    /// another call is still writing. The counter closes that window; the PID
    /// closes the equivalent one across processes.
    ///
    /// # Errors
    ///
    /// Returns an error if no candidate can be created exclusively.
    pub fn create_unique_file(dir: &Path, prefix: &str, suffix: &str) -> io::Result<(Self, File)> {
        for _ in 0..1_024 {
            let path = unique_path(dir, prefix, suffix);
            match File::options().write(true).create_new(true).open(&path) {
                Ok(file) => return Ok((Self::file(path), file)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve a unique temporary file",
        ))
    }

    /// Create and guard a unique file with an explicit Unix creation mode.
    ///
    /// On non-Unix platforms, `mode` is ignored.
    ///
    /// # Errors
    ///
    /// Returns an error if no candidate can be created exclusively.
    pub fn create_unique_file_with_mode(
        dir: &Path,
        prefix: &str,
        suffix: &str,
        mode: u32,
    ) -> io::Result<(Self, File)> {
        for _ in 0..1_024 {
            let path = unique_path(dir, prefix, suffix);
            #[cfg(unix)]
            let opened = {
                use std::os::unix::fs::OpenOptionsExt as _;

                File::options()
                    .write(true)
                    .create_new(true)
                    .mode(mode)
                    .open(&path)
            };
            #[cfg(not(unix))]
            let opened = {
                let _ = mode;
                File::options().write(true).create_new(true).open(&path)
            };
            match opened {
                Ok(file) => return Ok((Self::file(path), file)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve a unique temporary file",
        ))
    }

    /// Create and guard a new directory in `dir` whose name cannot collide
    /// with a concurrent call.
    ///
    /// # Errors
    ///
    /// Returns an error if no candidate can be created exclusively.
    pub fn create_unique_dir(dir: &Path, prefix: &str, suffix: &str) -> io::Result<Self> {
        for _ in 0..1_024 {
            let path = unique_path(dir, prefix, suffix);
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self::dir(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve a unique temporary directory",
        ))
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

fn unique_path(dir: &Path, prefix: &str, suffix: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let sequence = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    dir.join(format!(
        "{prefix}-{}-{sequence}-{suffix}",
        std::process::id()
    ))
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        match self.kind.remove(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn unique_file_with_mode_does_not_depend_on_umask() {
        let dir = tempfile::tempdir().unwrap();
        let (guard, file) =
            TempGuard::create_unique_file_with_mode(dir.path(), ".secret", "tmp", 0o600).unwrap();
        drop(file);

        let mode = std::fs::metadata(guard.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
