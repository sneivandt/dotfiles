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

/// A scratch directory for private files, including library-created intermediates.
#[derive(Debug)]
pub(crate) struct PrivateTempDir {
    // Drop the deny-delete handle before removing the directory tree.
    #[cfg(windows)]
    _pin: File,
    directory: TempGuard,
}

impl PrivateTempDir {
    pub(crate) fn create(parent: &Path, prefix: &str) -> anyhow::Result<Self> {
        #[cfg(unix)]
        {
            Ok(Self {
                directory: TempGuard::create_unique_dir_with_mode(parent, prefix, "tmp", 0o700)?,
            })
        }
        #[cfg(windows)]
        {
            let (directory, pin) = super::place::create_private_dir(parent, prefix)?;
            Ok(Self {
                _pin: pin,
                directory,
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (parent, prefix);
            anyhow::bail!("private staging directories are unsupported on this platform")
        }
    }

    pub(crate) fn path(&self) -> &Path {
        self.directory.path()
    }

    pub(crate) const fn persist(&mut self) {
        self.directory.persist();
    }

    pub(crate) fn create_file(&self, prefix: &str) -> anyhow::Result<(TempGuard, File)> {
        let (guard, file) =
            TempGuard::create_unique_file_with_mode(self.path(), prefix, "tmp", 0o600)?;
        #[cfg(windows)]
        super::place::verify_private_child(&file)?;
        Ok((guard, file))
    }
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
        Self::reserve_file(dir, prefix, suffix, &mut File::options())
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
        let mut options = File::options();
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(mode);
        }
        #[cfg(not(unix))]
        let _ = mode;
        Self::reserve_file(dir, prefix, suffix, &mut options)
    }

    fn reserve_file(
        dir: &Path,
        prefix: &str,
        suffix: &str,
        options: &mut std::fs::OpenOptions,
    ) -> io::Result<(Self, File)> {
        options.write(true).create_new(true);
        Self::reserve_file_with(dir, prefix, suffix, |path| options.open(path))
    }

    pub(super) fn reserve_file_with(
        dir: &Path,
        prefix: &str,
        suffix: &str,
        mut create_new: impl FnMut(&Path) -> io::Result<File>,
    ) -> io::Result<(Self, File)> {
        for _ in 0..1_024 {
            let path = unique_path(dir, prefix, suffix);
            match create_new(&path) {
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
        Self::create_unique_dir_with_mode(dir, prefix, suffix, 0o777)
    }

    /// Create and guard a unique directory with an explicit Unix creation mode.
    ///
    /// On non-Unix platforms, `mode` is ignored.
    ///
    /// # Errors
    ///
    /// Returns an error if no candidate can be created exclusively.
    pub fn create_unique_dir_with_mode(
        dir: &Path,
        prefix: &str,
        suffix: &str,
        mode: u32,
    ) -> io::Result<Self> {
        let builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt as _;
            let mut builder = builder;
            builder.mode(mode);
            builder
        };
        #[cfg(not(unix))]
        let _ = mode;
        Self::reserve_dir_with(dir, prefix, suffix, |path| builder.create(path))
            .map(|(guard, ())| guard)
    }

    pub(super) fn reserve_dir_with<T>(
        dir: &Path,
        prefix: &str,
        suffix: &str,
        mut create: impl FnMut(&Path) -> io::Result<T>,
    ) -> io::Result<(Self, T)> {
        for _ in 0..1_024 {
            let path = unique_path(dir, prefix, suffix);
            match create(&path) {
                Ok(value) => return Ok((Self::dir(path), value)),
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

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn cleanup_ignores_missing_paths_but_records_other_errors() {
        let (log, fixture, _guard) = crate::infra::logging::isolated_logger();
        let missing = fixture.path().join("already-clean");
        let wrong_kind = fixture.path().join("expected-file");
        std::fs::create_dir(&wrong_kind).unwrap();
        drop(TempGuard::file(missing));
        drop(TempGuard::file(wrong_kind.clone()));
        let records = std::fs::read_to_string(log.log_path().unwrap()).unwrap();
        let errors: Vec<_> = records
            .lines()
            .filter(|line| line.contains("failed to remove temporary file"))
            .collect();
        assert_eq!(errors.len(), 1, "{records}");
        assert!(errors[0].contains("expected-file"), "{errors:?}");
        assert!(wrong_kind.is_dir());
    }

    #[test]
    fn unique_file_variants_reserve_distinct_paths_and_clean_up() {
        let dir = tempfile::tempdir().unwrap();
        let (ordinary, ordinary_file) =
            TempGuard::create_unique_file(dir.path(), ".file", "tmp").unwrap();
        let (restricted, restricted_file) =
            TempGuard::create_unique_file_with_mode(dir.path(), ".file", "tmp", 0o600).unwrap();
        assert_ne!(ordinary.path(), restricted.path());
        assert!(ordinary.path().is_file());
        assert!(restricted.path().is_file());
        drop((ordinary_file, restricted_file));
        drop((ordinary, restricted));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn unique_file_variants_preserve_non_collision_errors() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        for error in [
            TempGuard::create_unique_file(&missing, ".file", "tmp").unwrap_err(),
            TempGuard::create_unique_file_with_mode(&missing, ".file", "tmp", 0o600).unwrap_err(),
            TempGuard::create_unique_dir(&missing, ".directory", "tmp").unwrap_err(),
        ] {
            assert_eq!(error.kind(), io::ErrorKind::NotFound);
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn reservation_retries_after_already_exists_collisions() {
        let dir = tempfile::tempdir().unwrap();
        let mut file_attempts = 0;
        let (file_guard, _file) =
            TempGuard::reserve_file_with(dir.path(), ".retry", "tmp", |path| {
                file_attempts += 1;
                if file_attempts == 1 {
                    return Err(io::Error::from(io::ErrorKind::AlreadyExists));
                }
                File::options().write(true).create_new(true).open(path)
            })
            .unwrap();
        assert_eq!(file_attempts, 2);
        drop(file_guard);

        let mut dir_attempts = 0;
        let (dir_guard, ()) = TempGuard::reserve_dir_with(dir.path(), ".retry", "tmp", |path| {
            dir_attempts += 1;
            if dir_attempts == 1 {
                return Err(io::Error::from(io::ErrorKind::AlreadyExists));
            }
            std::fs::create_dir(path)
        })
        .unwrap();
        assert_eq!(dir_attempts, 2);
        drop(dir_guard);
    }

    #[test]
    fn concurrent_reservations_keep_distinct_contents_until_their_own_guard_is_dropped() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let workers = 8;
        let barrier = std::sync::Barrier::new(workers);
        let files = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..workers)
                .map(|worker| {
                    let barrier = &barrier;
                    let dir = dir.path();
                    scope.spawn(move || {
                        barrier.wait();
                        let (guard, mut file) =
                            TempGuard::create_unique_file(dir, ".shared", "tmp").unwrap();
                        write!(file, "{worker}").unwrap();
                        drop(file);
                        (guard, worker.to_string())
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), workers);
        for (guard, content) in &files {
            assert_eq!(std::fs::read_to_string(guard.path()).unwrap(), *content);
        }
        drop(files);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
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

    #[test]
    fn private_temp_dir_cleans_up_by_default_and_retains_files_when_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let transient_path = {
            let transient = PrivateTempDir::create(dir.path(), ".transient").unwrap();
            let path = transient.path().to_path_buf();
            #[cfg(unix)]
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o700
            );
            #[cfg(windows)]
            crate::infra::fs::check_private_fixture(&path).unwrap();
            std::fs::write(path.join("entry"), "temporary").unwrap();
            path
        };
        assert!(!transient_path.exists());

        let persisted_path = {
            let mut persisted = PrivateTempDir::create(dir.path(), ".persisted").unwrap();
            let path = persisted.path().to_path_buf();
            std::fs::write(path.join("entry"), "keep me").unwrap();
            persisted.persist();
            path
        };
        assert!(persisted_path.is_dir());
        assert_eq!(
            std::fs::read_to_string(persisted_path.join("entry")).unwrap(),
            "keep me"
        );
    }
}
