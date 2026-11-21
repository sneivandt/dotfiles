//! Cross-process serialization for repositories and shared home destinations.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Seek as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use base64::Engine as _;
use serde::Serialize;
use sha2::{Digest as _, Sha256};

use crate::infra::env::Env;
use crate::infra::platform::Platform;

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Win32 file mapping and process snapshot for legacy updater compatibility"
)]
mod windows;

/// An exclusive lock held for the lifetime of one command run.
#[derive(Debug)]
pub(crate) struct RunLock {
    path: PathBuf,
    file: File,
}

#[derive(Serialize)]
struct Owner<'a> {
    pid: u32,
    started_unix_seconds: u64,
    command: &'a str,
}

mod coordination;

const LOCK_NAME: &str = "dotfiles-run.lock";
const FIRST_LOGIN_WAIT_ENV: &str = "DOTFILES_FIRST_LOGIN_WAIT_FOR_LOCK";
const RESTART_PREFIX: &str = ".dotfiles-restart";
const HOME_RESTART_PREFIX: &str = ".dotfiles-restart-home";
const COORDINATED_RESTART_PREFIX: &str = ".dotfiles-restart-home-coordinated";
const SHARED_RESTART_PREFIX: &str = ".dotfiles-restart-home-shared";

/// Locks held for a command and its continuation after a process restart.
#[derive(Debug)]
pub(crate) struct CommandLocks {
    repository_path: PathBuf,
    checkout_path: Option<PathBuf>,
    owned: Vec<RunLock>,
    legacy_overlay: Option<RunLock>,
    continuations: Vec<File>,
    share_originals: bool,
}

impl CommandLocks {
    pub(crate) fn acquire(
        root: &Path,
        home: &Path,
        env: &dyn Env,
        platform: Platform,
        command: &str,
    ) -> Result<Self> {
        let repository_path = repository_state_path(root, env, platform, LOCK_NAME)?;
        let wait = env.var(FIRST_LOGIN_WAIT_ENV);
        let repository =
            RunLock::acquire_path_with_wait(&repository_path, command, wait.as_deref())?;
        let home = RunLock::acquire_path_with_wait(
            &home_lock_path(home, platform)?,
            command,
            wait.as_deref(),
        )?;
        let checkout_path = checkout_coordination_path(root, &repository_path)?;
        let mut owned = vec![repository, home];
        if let Some(path) = &checkout_path {
            owned.push(RunLock::acquire_path_with_wait(
                path,
                command,
                wait.as_deref(),
            )?);
        }
        Ok(Self {
            repository_path,
            checkout_path,
            owned,
            legacy_overlay: None,
            continuations: Vec::new(),
            share_originals: true,
        })
    }

    pub(crate) fn restart(
        root: &Path,
        home: &Path,
        env: &dyn Env,
        platform: Platform,
        marker: &std::ffi::OsStr,
        command: &str,
    ) -> Result<Self> {
        verify_restart(root, env, platform, marker)?;
        let repository_path = repository_state_path(root, env, platform, LOCK_NAME)?;
        let home_path = home_lock_path(home, platform)?;
        let home_lock = if RestartLease::covers_home(marker) {
            anyhow::ensure!(
                is_run_held(&home_path)?,
                "restart has no active home lock. Retry the command"
            );
            None
        } else {
            // Older installed binaries hold only the repository lock.
            Some(RunLock::acquire_path(&home_path, command)?)
        };
        let mut continuations = vec![
            hold_continuation(&repository_path)?,
            hold_continuation(&home_path)?,
        ];
        let share_originals = RestartLease::covers_shared_base(marker);
        if share_originals {
            continuations.push(hold_shared(&repository_path, false)?);
            continuations.push(hold_shared(&home_path, false)?);
        }
        // Legacy parents retain exclusive base locks during their child's run.
        let checkout_path = checkout_coordination_path(root, &repository_path)?;
        let mut locks = Self {
            repository_path,
            checkout_path: checkout_path.clone(),
            owned: home_lock.into_iter().collect(),
            legacy_overlay: None,
            continuations,
            share_originals,
        };
        if let Some(path) = checkout_path {
            if RestartLease::covers_checkout(marker) {
                locks.continue_checkout_lock(&path)?;
            } else {
                // Older binaries do not acquire the account-local checkout lock.
                locks.owned.push(RunLock::acquire_path(&path, command)?);
            }
        }
        // The parent may have exited between validation and acquisition.
        verify_restart(root, env, platform, marker)?;
        Ok(locks)
    }

    pub(crate) fn protect_overlay(
        &mut self,
        overlay: &Path,
        env: &dyn Env,
        platform: Platform,
        command: &str,
        restart: Option<(&Path, &std::ffi::OsStr)>,
    ) -> Result<()> {
        if !has_git_marker(overlay)? {
            return Ok(());
        }
        let path = repository_state_path(overlay, env, platform, LOCK_NAME)?;
        let checkout_lock = coordination_path_for_git(&path)?;
        if self.checkout_path.as_ref() == Some(&checkout_lock)
            || crate::infra::fs::paths_equal(&self.repository_path, &path)?
        {
            return Ok(());
        }
        if let Some((root, marker)) = restart {
            verify_restart(root, env, platform, marker)?;
            self.continue_checkout_lock(&checkout_lock)?;
            match is_run_held(&path) {
                Ok(true) => self.continuations.push(hold_shared(&path, false)?),
                Ok(false) => {}
                Err(error) if unavailable_optional_lock(&error) => {}
                Err(error) => return Err(error),
            }
            verify_restart(root, env, platform, marker)?;
        } else {
            // Fail fast so opposite main/overlay pairs cannot wait on each other.
            // The account-local lock also covers read-only views of this checkout.
            self.owned
                .push(RunLock::acquire_path(&checkout_lock, command)?);
            match RunLock::acquire_path(&path, command) {
                Ok(lock) => self.legacy_overlay = Some(lock),
                Err(error) if unavailable_optional_lock(&error) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn continue_checkout_lock(&mut self, path: &Path) -> Result<()> {
        anyhow::ensure!(
            is_run_held(path)?,
            "restart has no active checkout lock at {}. Retry the command",
            path.display()
        );
        self.continuations.push(hold_continuation(path)?);
        self.continuations.push(hold_shared(path, false)?);
        Ok(())
    }

    /// Keep the original lock paths occupied by both processes. Older
    /// contenders know only these paths and request exclusive access.
    pub(crate) fn prepare_restart(&self) -> Result<(Vec<File>, RestartLockMode)> {
        let continuations = self
            .owned
            .iter()
            .map(|lock| hold_continuation(&lock.path))
            .collect::<Result<Vec<_>>>()?;
        // The overlay's account-local lock covers this handoff, even when its
        // Git directory permits opening the original but forbids creating a file.
        for lock in self.owned.iter().chain(self.legacy_overlay.as_ref()) {
            // A legacy contender can win between unlock and the shared
            // acquisition. Fail before starting the successor in that case.
            lock.file
                .unlock()
                .with_context(|| format!("releasing exclusive lock {}", lock.path.display()))?;
            take_shared(&lock.file, &lock.path)?;
        }
        let mode = if self.share_originals {
            RestartLockMode::Shared
        } else {
            RestartLockMode::Coordinated
        };
        Ok((continuations, mode))
    }
}

fn has_git_marker(root: &Path) -> Result<bool> {
    Ok(
        crate::infra::fs::symlink_metadata_optional(&root.join(".git"), "inspect checkout marker")?
            .is_some(),
    )
}

fn checkout_coordination_path(root: &Path, repository_path: &Path) -> Result<Option<PathBuf>> {
    if !has_git_marker(root)? {
        return Ok(None);
    }
    coordination_path_for_git(repository_path).map(Some)
}

fn coordination_path_for_git(repository_path: &Path) -> Result<PathBuf> {
    let common_dir = repository_path
        .parent()
        .context("missing Git common directory")?;
    coordination::checkout_lock_path(common_dir)
}

fn unavailable_optional_lock(error: &anyhow::Error) -> bool {
    error.downcast_ref::<std::io::Error>().is_some_and(|cause| {
        matches!(
            cause.kind(),
            std::io::ErrorKind::PermissionDenied
                | std::io::ErrorKind::ReadOnlyFilesystem
                | std::io::ErrorKind::NotFound
        )
    })
}

fn continuation_path(path: &Path) -> PathBuf {
    path.with_extension("continuation.lock")
}

fn hold_continuation(path: &Path) -> Result<File> {
    hold_shared(&continuation_path(path), true)
}

fn hold_shared(path: &Path, create: bool) -> Result<File> {
    let file = open_lock(path, create)?;
    take_shared(&file, path)?;
    Ok(file)
}

fn take_shared(file: &File, path: &Path) -> Result<()> {
    file.try_lock_shared()
        .map_err(|error| lock_error(path, error))
}

fn ensure_no_continuation(path: &Path, wait: Option<&str>) -> Result<()> {
    let path = continuation_path(path);
    if crate::infra::fs::symlink_metadata_optional(&path, "inspect restart continuation lock")?
        .is_some()
    {
        take_exclusive(&open_lock(&path, false)?, &path, wait)?;
    }
    Ok(())
}

fn home_lock_path(home: &Path, platform: Platform) -> Result<PathBuf> {
    anyhow::ensure!(
        home.is_absolute(),
        "home lock requires an absolute home directory"
    );
    // Anchor coordination in the destination, independent of the checkout and
    // XDG_STATE_HOME/LOCALAPPDATA overrides. Resolve aliases of the home too.
    let home = crate::infra::fs::resolve_directory(home)?;
    let state = if platform.is_windows() {
        home.join("AppData").join("Local")
    } else {
        home.join(".local").join("state")
    };
    Ok(state.join("dotfiles").join("home-run.lock"))
}

/// A repository-scoped lock held by the process that starts a restart chain.
pub(crate) struct RestartLease {
    _file: File,
    marker: crate::infra::fs::TempGuard,
}

/// Which lock handles the immediate restart parent can share with its child.
#[derive(Debug, Clone, Copy)]
pub(crate) enum RestartLockMode {
    Legacy,
    Coordinated,
    Shared,
}

impl RestartLease {
    pub(crate) fn covers_home(marker: &std::ffi::OsStr) -> bool {
        marker
            .as_encoded_bytes()
            .starts_with(b".dotfiles-restart-home-")
    }

    fn covers_shared_base(marker: &std::ffi::OsStr) -> bool {
        marker
            .as_encoded_bytes()
            .starts_with(b".dotfiles-restart-home-shared-")
    }

    fn covers_checkout(marker: &std::ffi::OsStr) -> bool {
        Self::covers_shared_base(marker)
            || marker
                .as_encoded_bytes()
                .starts_with(b".dotfiles-restart-home-coordinated-")
    }

    pub(crate) fn new(
        root: &Path,
        env: &dyn Env,
        platform: Platform,
        mode: RestartLockMode,
    ) -> Result<Self> {
        let path = repository_state_path(root, env, platform, LOCK_NAME)?;
        let parent = path.parent().context("missing run-lock directory")?;
        let (marker, file) = crate::infra::fs::TempGuard::create_unique_file_with_mode(
            parent,
            match mode {
                RestartLockMode::Legacy => HOME_RESTART_PREFIX,
                RestartLockMode::Coordinated => COORDINATED_RESTART_PREFIX,
                RestartLockMode::Shared => SHARED_RESTART_PREFIX,
            },
            "lock",
            0o600,
        )?;
        file.try_lock()
            .map_err(|error| lock_error(marker.path(), error))?;
        Ok(Self {
            _file: file,
            marker,
        })
    }

    pub(crate) fn marker(&self) -> Result<&std::ffi::OsStr> {
        self.marker
            .path()
            .file_name()
            .context("missing restart lease name")
    }
}

fn is_held(path: &Path) -> Result<bool> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(false),
        Err(TryLockError::WouldBlock) => Ok(true),
        Err(error) => Err(lock_error(path, error)),
    }
}

fn is_run_held(path: &Path) -> Result<bool> {
    if is_held(path)? {
        return Ok(true);
    }
    let continuation = continuation_path(path);
    if crate::infra::fs::symlink_metadata_optional(&continuation, "inspect continuation lock")?
        .is_none()
    {
        return Ok(false);
    }
    is_held(&continuation)
}

pub(crate) fn verify_restart(
    root: &Path,
    env: &dyn Env,
    platform: Platform,
    marker: &std::ffi::OsStr,
) -> Result<()> {
    let lock = repository_state_path(root, env, platform, LOCK_NAME)?;
    let held = if RestartLease::covers_home(marker) {
        is_run_held(&lock)?
    } else {
        is_held(&lock)?
    };
    anyhow::ensure!(
        held,
        "restart has no active repository lock. Retry the command"
    );
    // Existing release binaries pass a literal 1. Match their recorded owner
    // to the actual parent, while still requiring the original lock above.
    #[cfg(any(unix, windows))]
    if marker == "1" {
        #[cfg(unix)]
        let (bytes, parent) = (
            std::fs::read(&lock)?,
            i64::from(nix::unistd::getppid().as_raw()),
        );
        #[cfg(windows)]
        let (bytes, parent) = {
            let (bytes, parent) = windows::legacy_owner(&lock)?;
            (bytes, i64::from(parent))
        };
        let owner: serde_json::Value = serde_json::from_slice(&bytes)?;
        anyhow::ensure!(
            owner.get("pid").and_then(serde_json::Value::as_i64) == Some(parent),
            "restart parent does not own the repository lock. Retry the command"
        );
        return Ok(());
    }
    let name = marker.to_str().context("invalid restart lease")?;
    anyhow::ensure!(
        name.starts_with(RESTART_PREFIX)
            && name.ends_with("-lock")
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')),
        "restart has no verifiable lease. Retry the command"
    );
    let lease = lock.with_file_name(name);
    anyhow::ensure!(
        is_held(&lease)?,
        "restart lease has expired. Retry the command"
    );
    Ok(())
}

impl RunLock {
    /// Acquire the lock associated with `root`.
    #[cfg(test)]
    pub(crate) fn acquire(
        root: &Path,
        env: &dyn Env,
        platform: Platform,
        command: &str,
    ) -> Result<Self> {
        let path = repository_state_path(root, env, platform, LOCK_NAME)?;
        Self::acquire_path_with_wait(&path, command, env.var(FIRST_LOGIN_WAIT_ENV).as_deref())
    }

    pub(crate) fn acquire_path(path: &Path, command: &str) -> Result<Self> {
        Self::acquire_path_with_wait(path, command, None)
    }

    fn acquire_path_with_wait(path: &Path, command: &str, wait: Option<&str>) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating run-lock directory {}", parent.display()))?;
        }

        let mut file = open_lock(path, true)?;
        take_exclusive(&file, path, wait)?;
        ensure_no_continuation(path, wait)?;

        let started_unix_seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before the Unix epoch")?
            .as_secs();
        let owner = Owner {
            pid: std::process::id(),
            started_unix_seconds,
            command,
        };
        let contents = serde_json::to_vec(&owner).context("serializing run-lock owner")?;
        file.set_len(0)
            .with_context(|| format!("clearing run lock {}", path.display()))?;
        file.seek(std::io::SeekFrom::Start(0))
            .with_context(|| format!("rewinding run lock {}", path.display()))?;
        file.write_all(&contents)
            .with_context(|| format!("writing run lock {}", path.display()))?;
        file.sync_data()
            .with_context(|| format!("syncing run lock {}", path.display()))?;

        Ok(Self {
            file,
            path: path.to_path_buf(),
        })
    }
}

fn open_lock(path: &Path, create: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options
        .create(create)
        .read(true)
        .write(true)
        .truncate(false);
    let file = crate::infra::fs::open_in_place(path, &mut options)
        .with_context(|| format!("opening run lock {}", path.display()))?;
    #[cfg(unix)]
    check_owner(&file.metadata()?, nix::unistd::Uid::effective().as_raw())
        .with_context(|| format!("validating run lock {}", path.display()))?;
    Ok(file)
}

fn take_exclusive(file: &File, path: &Path, wait: Option<&str>) -> Result<()> {
    if matches!(wait, Some("1")) {
        file.lock()
            .with_context(|| format!("waiting for run lock {}", path.display()))
    } else {
        file.try_lock().map_err(|error| lock_error(path, error))
    }
}

#[cfg(unix)]
fn check_owner(metadata: &std::fs::Metadata, expected_uid: u32) -> Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    anyhow::ensure!(
        metadata.uid() == expected_uid,
        "run lock is not owned by the current user"
    );
    Ok(())
}

fn lock_error(path: &Path, error: TryLockError) -> anyhow::Error {
    match error {
        TryLockError::Error(error) => {
            anyhow::Error::new(error).context(format!("locking run lock {}", path.display()))
        }
        TryLockError::WouldBlock => {
            // Owner text is advisory; Windows can deny reads of the locked file.
            let owner = std::fs::read_to_string(path).unwrap_or_default();
            let owner = owner.trim();
            let detail = if owner.is_empty() {
                String::new()
            } else {
                format!(": {owner}")
            };
            anyhow::anyhow!(
                "another dotfiles run holds {}{detail}; wait for it to finish and retry",
                path.display()
            )
        }
    }
}

impl Drop for RunLock {
    fn drop(&mut self) {
        drop(self.file.unlock());
    }
}

/// Resolve a repository-scoped internal state path.
///
/// Blank or relative state/home environment values are ignored so fallback
/// lock identities never depend on the command's working directory.
pub(crate) fn repository_state_path(
    root: &Path,
    env: &dyn Env,
    platform: Platform,
    filename: &str,
) -> Result<PathBuf> {
    // Exported dotfiles trees can live inside an unrelated repository.
    // Only the selected root's Git metadata can own its shared lock.
    if has_git_marker(root)? {
        let repository = git2::Repository::open(root)
            .with_context(|| format!("opening selected repository {}", root.display()))?;
        return Ok(repository.commondir().join(filename));
    }

    let state_root = if platform.is_windows() {
        absolute_env_path(env, "LOCALAPPDATA").or_else(|| {
            absolute_env_path(env, "USERPROFILE").map(|home| home.join("AppData").join("Local"))
        })
    } else {
        absolute_env_path(env, "XDG_STATE_HOME").or_else(|| {
            absolute_env_path(env, "HOME").map(|home| home.join(".local").join("state"))
        })
    }
    .context("cannot determine platform state directory for the run lock")?;

    let digest = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(root.as_os_str().as_encoded_bytes()));
    Ok(state_root
        .join("dotfiles")
        .join("repositories")
        .join(digest)
        .join(filename))
}

fn absolute_env_path(env: &dyn Env, key: &str) -> Option<PathBuf> {
    env.var_os(key)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::env::MapEnv;
    use crate::infra::platform::{Os, Platform};
    use std::io::Read as _;

    #[cfg(unix)]
    #[test]
    fn lock_owner_validation_rejects_another_users_file_without_writing() {
        let fixture = tempfile::tempdir().unwrap();
        let target = fixture.path().join("lock");
        std::fs::write(&target, "foreign owner data").unwrap();
        let metadata = target.metadata().unwrap();
        let uid = nix::unistd::Uid::effective().as_raw();
        check_owner(&metadata, uid).unwrap();
        assert!(check_owner(&metadata, uid.wrapping_add(1)).is_err());
        assert_eq!(
            std::fs::read_to_string(target).unwrap(),
            "foreign owner data"
        );
    }

    #[test]
    fn lock_rejects_hard_links_and_retries_after_the_alias_is_removed() {
        let fixture = tempfile::tempdir().unwrap();
        let target = fixture.path().join("unrelated");
        let lock = fixture.path().join("run.lock");
        std::fs::write(&target, "unrelated user data").unwrap();
        std::fs::hard_link(&target, &lock).unwrap();
        let error = RunLock::acquire_path(&lock, "check").unwrap_err();
        assert!(
            format!("{error:#}").contains("multiply linked"),
            "{error:#}"
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "unrelated user data"
        );
        std::fs::remove_file(&lock).unwrap();
        let holder = RunLock::acquire_path(&lock, "check").unwrap();
        assert!(RunLock::acquire_path(&lock, "update").is_err());
        drop(holder);
        drop(RunLock::acquire_path(&lock, "update").unwrap());
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "unrelated user data"
        );
    }

    #[test]
    fn independent_checkouts_share_home_locks_even_with_different_state_overrides() {
        let fixture = tempfile::tempdir().unwrap();
        let first = fixture.path().join("first");
        let second = fixture.path().join("second");
        git2::Repository::init(&first).unwrap();
        git2::Repository::init(&second).unwrap();
        for os in [Os::Linux, Os::Windows] {
            let platform = Platform::new(os, false);
            let home = fixture.path().join(os.to_string());
            let env = MapEnv::new();
            let holder = CommandLocks::acquire(&first, &home, &env, platform, "update").unwrap();
            let other_env = MapEnv::new()
                .with("XDG_STATE_HOME", fixture.path().join("different-state"))
                .with("LOCALAPPDATA", fixture.path().join("different-local"));
            let home_conflict =
                CommandLocks::acquire(&second, &home, &other_env, platform, "remove").unwrap_err();
            assert!(
                home_conflict.to_string().contains("home-run.lock"),
                "{home_conflict:#}"
            );
            assert!(
                home_conflict.to_string().contains("another dotfiles run"),
                "{home_conflict:#}"
            );

            // A failed home-lock acquisition must release its repository lock.
            let other_home = fixture.path().join(format!("other-{os}"));
            let independent =
                CommandLocks::acquire(&second, &other_home, &env, platform, "check").unwrap();
            let repository_conflict =
                CommandLocks::acquire(&first, &other_home, &env, platform, "update").unwrap_err();
            assert!(
                repository_conflict
                    .to_string()
                    .contains("dotfiles-run.lock"),
                "{repository_conflict:#}"
            );
            drop((holder, independent));
            CommandLocks::acquire(&second, &home, &env, platform, "retry").unwrap();
        }
    }

    #[test]
    fn exported_overlay_needs_no_git_lock_storage() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("main");
        let overlay = fixture.path().join("export");
        let home = fixture.path().join("home");
        git2::Repository::init(&root).unwrap();
        std::fs::create_dir(&overlay).unwrap();
        let env = MapEnv::new().with("HOME", home.as_os_str());
        let windows = Platform::new(Os::Windows, false);
        let mut locks = CommandLocks::acquire(&root, &home, &env, windows, "check").unwrap();
        locks
            .protect_overlay(&overlay, &env, windows, "check", None)
            .unwrap();
        assert!(std::fs::read_dir(&overlay).unwrap().next().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn read_only_overlay_coordinates_with_a_writable_checkout_alias() {
        use std::os::unix::fs::{PermissionsExt as _, symlink};

        if nix::unistd::Uid::effective().is_root() {
            return;
        }
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("root");
        let overlay = fixture.path().join("overlay");
        let alias = fixture.path().join("alias");
        let home = fixture.path().join("home");
        let other_home = fixture.path().join("other-home");
        for repository in [&root, &overlay] {
            git2::Repository::init(repository).unwrap();
        }
        symlink(&overlay, &alias).unwrap();

        let platform = Platform::detect();
        let env = MapEnv::new();
        let mut active = CommandLocks::acquire(&root, &home, &env, platform, "preview").unwrap();
        let git_dir = overlay.join(".git");
        std::fs::set_permissions(&git_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let protection = active.protect_overlay(&overlay, &env, platform, "preview", None);
        std::fs::set_permissions(&git_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        protection.unwrap();
        assert!(!git_dir.join(LOCK_NAME).exists());

        let conflict =
            CommandLocks::acquire(&alias, &other_home, &env, platform, "update").unwrap_err();
        assert!(
            conflict.to_string().contains("another dotfiles run"),
            "{conflict:#}"
        );
        drop(active);
        drop(CommandLocks::acquire(&alias, &other_home, &env, platform, "retry").unwrap());

        // A prior writable run leaves an original lock file. A later restart
        // must preserve its protection without creating another Git file.
        assert!(git_dir.join(LOCK_NAME).is_file());
        let mut parent = CommandLocks::acquire(&root, &home, &env, platform, "update").unwrap();
        std::fs::set_permissions(&git_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let mut restart = || -> Result<()> {
            parent.protect_overlay(&overlay, &env, platform, "update", None)?;
            let (_guards, mode) = parent.prepare_restart()?;
            let lease = RestartLease::new(&root, &env, platform, mode)?;
            let marker = lease.marker()?;
            let mut child = CommandLocks::restart(&root, &home, &env, platform, marker, "update")?;
            child.protect_overlay(&overlay, &env, platform, "update", Some((&root, marker)))
        };
        let result = restart();
        std::fs::set_permissions(&git_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        result.unwrap();
        assert!(!continuation_path(&git_dir.join(LOCK_NAME)).exists());
    }

    #[test]
    fn first_login_waits_for_the_repository_or_home_lock() {
        use std::sync::mpsc::{RecvTimeoutError, channel};
        use std::time::Duration;

        let fixture = tempfile::tempdir().unwrap();
        let first = fixture.path().join("first");
        let second = fixture.path().join("second");
        let home = fixture.path().join("home");
        git2::Repository::init(&first).unwrap();
        git2::Repository::init(&second).unwrap();
        let platform = Platform::new(Os::Linux, false);
        let env = MapEnv::new();
        let service_env = MapEnv::new().with("DOTFILES_FIRST_LOGIN_WAIT_FOR_LOCK", "1");

        for root in [&first, &second] {
            std::thread::scope(|scope| {
                let holder =
                    CommandLocks::acquire(&first, &home, &env, platform, "update").unwrap();
                let (started, start) = channel();
                let (finished, finish) = channel();
                let home = &home;
                let service_env = &service_env;
                scope.spawn(move || {
                    started.send(()).unwrap();
                    let result =
                        CommandLocks::acquire(root, home, service_env, platform, "first login");
                    finished.send(result).unwrap();
                });
                start.recv().unwrap();
                let attempt = finish.recv_timeout(Duration::from_millis(100));
                drop(holder);
                assert!(matches!(attempt, Err(RecvTimeoutError::Timeout)));
                drop(
                    finish
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap()
                        .unwrap(),
                );
            });
        }
    }

    #[cfg(unix)]
    #[test]
    fn home_aliases_contend_for_the_same_destination_lock() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let alias = fixture.path().join("alias");
        std::os::unix::fs::symlink(&home, &alias).unwrap();
        let platform = Platform::detect();
        let path = home_lock_path(&home, platform).unwrap();
        assert_eq!(home_lock_path(&alias, platform).unwrap(), path);
        let _lock = RunLock::acquire_path(&path, "update").unwrap();
        let error = RunLock::acquire_path(&home_lock_path(&alias, platform).unwrap(), "remove")
            .unwrap_err();
        assert!(error.to_string().contains("another dotfiles run"));
    }

    #[test]
    fn command_restarts_require_the_parent_home_lock_and_live_repository_lease() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("repository");
        git2::Repository::init(&root).unwrap();
        let home = fixture.path().join("home");
        let env = MapEnv::new();
        let platform = Platform::detect();
        let locks = CommandLocks::acquire(&root, &home, &env, platform, "update").unwrap();
        let (continuations, mode) = locks.prepare_restart().unwrap();
        let lease = RestartLease::new(&root, &env, platform, mode).unwrap();
        let marker = lease.marker().unwrap().to_owned();
        let child = CommandLocks::restart(&root, &home, &env, platform, &marker, "update").unwrap();
        drop(child);
        drop((locks, continuations));
        let _repository = RunLock::acquire(&root, &env, platform, "fixture").unwrap();
        let error =
            CommandLocks::restart(&root, &home, &env, platform, &marker, "update").unwrap_err();
        assert!(
            error.to_string().contains("no active home lock"),
            "{error:#}"
        );
        let _home =
            RunLock::acquire_path(&home_lock_path(&home, platform).unwrap(), "fixture").unwrap();
        drop(lease);
        assert!(CommandLocks::restart(&root, &home, &env, platform, &marker, "update").is_err());
    }

    #[test]
    fn legacy_repository_leases_acquire_home_lock_before_applying_configuration() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("repository");
        git2::Repository::init(&root).unwrap();
        let home = fixture.path().join("home");
        let env = MapEnv::new();
        let platform = Platform::detect();
        let _repository = RunLock::acquire(&root, &env, platform, "old binary").unwrap();
        let (marker, lease) = crate::infra::fs::TempGuard::create_unique_file_with_mode(
            &root.join(".git"),
            RESTART_PREFIX,
            "lock",
            0o600,
        )
        .unwrap();
        lease.try_lock().unwrap();
        let marker_name = marker.path().file_name().unwrap();
        let home_lock =
            CommandLocks::restart(&root, &home, &env, platform, marker_name, "update").unwrap();
        assert!(
            RunLock::acquire_path(&home_lock_path(&home, platform).unwrap(), "other clone")
                .is_err()
        );
        let (continuations, mode) = home_lock.prepare_restart().unwrap();
        let modern = RestartLease::new(&root, &env, platform, mode).unwrap();
        drop(
            CommandLocks::restart(
                &root,
                &home,
                &env,
                platform,
                modern.marker().unwrap(),
                "update",
            )
            .unwrap(),
        );
        drop((home_lock, continuations));
        let unrelated =
            RunLock::acquire_path(&home_lock_path(&home, platform).unwrap(), "other clone")
                .unwrap();
        assert!(
            CommandLocks::restart(&root, &home, &env, platform, marker_name, "update").is_err()
        );
        drop(unrelated);
        CommandLocks::restart(&root, &home, &env, platform, marker_name, "retry").unwrap();
    }

    #[test]
    fn shared_restart_rejects_a_legacy_lock_winner_before_work_can_resume() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("repository");
        let home = fixture.path().join("home");
        git2::Repository::init(&root).unwrap();
        let env = MapEnv::new();
        let platform = Platform::detect();
        let parent = CommandLocks::acquire(&root, &home, &env, platform, "update").unwrap();
        let (continuation, mode) = parent.prepare_restart().unwrap();
        let lease = RestartLease::new(&root, &env, platform, mode).unwrap();
        let marker = lease.marker().unwrap();
        drop(parent);

        // Simulate an older invocation taking the base lock while the
        // continuation and lease are still present.
        let repository = repository_state_path(&root, &env, platform, LOCK_NAME).unwrap();
        let legacy = open_lock(&repository, false).unwrap();
        legacy.try_lock().unwrap();
        let error =
            CommandLocks::restart(&root, &home, &env, platform, marker, "update").unwrap_err();
        assert!(
            error.to_string().contains("another dotfiles run"),
            "{error:#}"
        );
        drop(legacy);
        drop(continuation);
    }

    #[test]
    fn nested_continuation_keeps_the_home_and_checkouts_locked_after_parents_exit() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("repository");
        let overlay = fixture.path().join("overlay");
        let another_root = fixture.path().join("another");
        let home = fixture.path().join("home");
        let another_home = fixture.path().join("another-home");
        for repository in [&root, &overlay, &another_root] {
            git2::Repository::init(repository).unwrap();
        }
        let env = MapEnv::new();
        let platform = Platform::detect();
        let parent = CommandLocks::acquire(&root, &home, &env, platform, "update").unwrap();
        let (parent_continuation, parent_mode) = parent.prepare_restart().unwrap();
        let parent_lease = RestartLease::new(&root, &env, platform, parent_mode).unwrap();

        // Binary self-update takes place before selecting the overlay lock.
        let mut middle = CommandLocks::restart(
            &root,
            &home,
            &env,
            platform,
            parent_lease.marker().unwrap(),
            "update",
        )
        .unwrap();
        middle
            .protect_overlay(&overlay, &env, platform, "update", None)
            .unwrap();
        drop((parent, parent_lease, parent_continuation));
        let (middle_continuation, middle_mode) = middle.prepare_restart().unwrap();
        let middle_lease = RestartLease::new(&root, &env, platform, middle_mode).unwrap();

        // Repository synchronization restarts again with the selected overlay.
        let marker = middle_lease.marker().unwrap();
        let mut child =
            CommandLocks::restart(&root, &home, &env, platform, marker, "update").unwrap();
        child
            .protect_overlay(&overlay, &env, platform, "update", Some((&root, marker)))
            .unwrap();
        drop((middle, middle_lease, middle_continuation));

        for (repo, destination) in [
            (&root, &another_home),
            (&another_root, &home),
            (&overlay, &another_home),
        ] {
            let error =
                CommandLocks::acquire(repo, destination, &env, platform, "remove").unwrap_err();
            assert!(
                error.to_string().contains("another dotfiles run"),
                "{error:#}"
            );
            assert!(error.to_string().contains(".lock"), "{error:#}");
        }
        drop(child);
        for (repo, destination) in [
            (&root, &another_home),
            (&another_root, &home),
            (&overlay, &another_home),
        ] {
            CommandLocks::acquire(repo, destination, &env, platform, "retry").unwrap();
        }
    }

    #[test]
    fn home_lock_resolution_rejects_relative_paths_and_preserves_io_errors() {
        let platform = Platform::detect();
        assert!(home_lock_path(Path::new("relative-home"), platform).is_err());
        let fixture = tempfile::tempdir().unwrap();
        let invalid = fixture.path().join("invalid\0home");
        assert!(home_lock_path(&invalid, platform).is_err());
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn lock_rejects_a_symlink_without_writing_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("local-file");
        let lock = dir.path().join("lock");
        std::fs::write(&target, "local data").unwrap();
        std::os::unix::fs::symlink(&target, &lock).unwrap();

        assert!(RunLock::acquire_path(&lock, "fixture").is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "local data");
    }

    #[test]
    fn restart_requires_both_a_live_lease_and_its_repository_lock() {
        let root = tempfile::tempdir().unwrap();
        git2::Repository::init(root.path()).unwrap();
        let env = MapEnv::new();
        let platform = Platform::detect();
        let lock = RunLock::acquire(root.path(), &env, platform, "update").unwrap();
        let lease =
            RestartLease::new(root.path(), &env, platform, RestartLockMode::Legacy).unwrap();
        let marker = lease.marker().unwrap().to_owned();
        verify_restart(root.path(), &env, platform, &marker).unwrap();
        for invalid in [
            "",
            "../.dotfiles-restart-lock",
            ".dotfiles-restart-fake-lock",
        ] {
            assert!(verify_restart(root.path(), &env, platform, invalid.as_ref()).is_err());
        }
        drop(lock);
        assert!(
            verify_restart(root.path(), &env, platform, &marker)
                .unwrap_err()
                .to_string()
                .contains("no active repository lock")
        );
        let path = repository_state_path(root.path(), &env, platform, LOCK_NAME).unwrap();
        let _continuation = hold_continuation(&path).unwrap();
        verify_restart(root.path(), &env, platform, &marker).unwrap();
        drop(lease);
        assert!(verify_restart(root.path(), &env, platform, &marker).is_err());
    }

    #[test]
    fn operational_lock_errors_are_not_reported_as_contention() {
        let path = Path::new("fixture.lock");
        for kind in [
            std::io::ErrorKind::Unsupported,
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::Other,
        ] {
            let error = lock_error(
                path,
                TryLockError::Error(std::io::Error::new(kind, "fixture failure")),
            );
            assert_eq!(error.to_string(), "locking run lock fixture.lock");
            assert_eq!(error.downcast_ref::<std::io::Error>().unwrap().kind(), kind);
            assert!(!format!("{error:#}").contains("another dotfiles run"));
        }
    }

    #[test]
    fn prevents_overlapping_runs_and_releases_on_drop() {
        let root = tempfile::tempdir().expect("tempdir");
        git2::Repository::init(root.path()).expect("init repository");
        let env = MapEnv::new();

        let platform = Platform::new(Os::Windows, false);
        let mut first =
            RunLock::acquire(root.path(), &env, platform, "install").expect("first lock");
        let path = repository_state_path(root.path(), &env, platform, "dotfiles-run.lock").unwrap();
        // Windows excludes other handles from reading the locked region.
        first.file.rewind().unwrap();
        let mut owner_before = String::new();
        first.file.read_to_string(&mut owner_before).unwrap();
        let owner: serde_json::Value = serde_json::from_str(&owner_before).unwrap();
        assert_eq!(owner["pid"], std::process::id());
        assert_eq!(owner["command"], "install");
        assert!(owner["started_unix_seconds"].as_u64().unwrap() > 0);
        let error =
            RunLock::acquire(root.path(), &env, platform, "update").expect_err("second lock");
        assert!(
            error.to_string().contains("another dotfiles run"),
            "unexpected error: {error:#}"
        );
        assert!(error.to_string().contains(&path.display().to_string()));
        assert!(
            error
                .to_string()
                .contains("wait for it to finish and retry")
        );
        #[cfg(not(windows))]
        assert!(
            error.to_string().contains(&owner_before),
            "contention should identify the current owner"
        );
        drop(first);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            owner_before,
            "a failed contender must not truncate the holder's metadata"
        );

        let next =
            RunLock::acquire(root.path(), &env, platform, "check").expect("lock after release");
        drop(next);
        let replacement_owner: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            replacement_owner["command"], "check",
            "replace the longer stale owner record"
        );
    }

    #[test]
    fn linked_worktrees_share_a_lock_but_independent_repositories_do_not() {
        let root = tempfile::tempdir().unwrap();
        let repository = git2::Repository::init(root.path().join("main")).unwrap();
        let tree_id = repository.index().unwrap().write_tree().unwrap();
        let tree = repository.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("Test", "test@test.local").unwrap();
        repository
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .unwrap();
        let linked = root.path().join("linked");
        repository.worktree("linked", &linked, None).unwrap();
        let independent = root.path().join("independent");
        git2::Repository::init(&independent).unwrap();
        let env = MapEnv::new();
        let platform = Platform::new(Os::Linux, false);
        let _main =
            RunLock::acquire(repository.workdir().unwrap(), &env, platform, "install").unwrap();

        let error = RunLock::acquire(&linked, &env, platform, "update").unwrap_err();
        assert!(error.to_string().contains("another dotfiles run"));
        let _independent = RunLock::acquire(&independent, &env, platform, "check").unwrap();
    }

    #[test]
    fn exported_tree_never_replaces_ancestor_lock_or_falls_back_on_bad_metadata() {
        let fixture = tempfile::tempdir().unwrap();
        let outer = git2::Repository::init(fixture.path()).unwrap();
        let outer_lock = outer.commondir().join(LOCK_NAME);
        std::fs::write(&outer_lock, "outer state").unwrap();
        let export = fixture.path().join("export");
        std::fs::create_dir(&export).unwrap();
        let env = MapEnv::new().with("HOME", fixture.path().join("fixture-home"));
        let platform = Platform::new(Os::Linux, false);
        let marker = export.join(".git");
        std::fs::write(&marker, "gitdir: unavailable\n").unwrap();
        assert!(RunLock::acquire(&export, &env, platform, "check").is_err());
        assert!(!fixture.path().join("fixture-home").exists());
        std::fs::remove_file(marker).unwrap();
        let holder = RunLock::acquire(&export, &env, platform, "check").unwrap();
        assert!(RunLock::acquire(&export, &env, platform, "retry").is_err());
        drop(holder);
        drop(RunLock::acquire(&export, &env, platform, "retry").unwrap());
        assert_eq!(std::fs::read(outer_lock).unwrap(), b"outer state");
    }

    #[test]
    fn non_repository_state_paths_use_platform_precedence_and_repository_identity() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        for path in [&first, &second] {
            std::fs::create_dir(path).unwrap();
        }
        let local = root.path().join("local");
        let state = root.path().join("state");
        let home = root.path().join("home");
        for (name, os, env, base) in [
            (
                "windows explicit",
                Os::Windows,
                MapEnv::new()
                    .with("LOCALAPPDATA", &local)
                    .with("USERPROFILE", &home)
                    .with("XDG_STATE_HOME", &state),
                local,
            ),
            (
                "windows fallback",
                Os::Windows,
                MapEnv::new().with("USERPROFILE", &home),
                home.join("AppData").join("Local"),
            ),
            (
                "linux explicit",
                Os::Linux,
                MapEnv::new()
                    .with("XDG_STATE_HOME", &state)
                    .with("HOME", &home),
                state,
            ),
            (
                "linux fallback",
                Os::Linux,
                MapEnv::new().with("HOME", &home),
                home.join(".local").join("state"),
            ),
        ] {
            let platform = Platform::new(os, false);
            let first_path = repository_state_path(&first, &env, platform, "lock").unwrap();
            let second_path = repository_state_path(&second, &env, platform, "lock").unwrap();
            assert!(
                first_path.starts_with(base.join("dotfiles").join("repositories")),
                "{name}"
            );
            assert_eq!(first_path.file_name().unwrap(), "lock", "{name}");
            assert_ne!(
                first_path, second_path,
                "{name}: repositories must not collide"
            );
            assert_eq!(
                repository_state_path(&first, &env, platform, "state")
                    .unwrap()
                    .parent(),
                first_path.parent(),
                "{name}: all state files share a repository namespace"
            );
            assert!(
                !base.exists(),
                "{name}: path resolution must not create state"
            );
        }
        for os in [Os::Linux, Os::Windows] {
            let error =
                repository_state_path(&first, &MapEnv::new(), Platform::new(os, false), "lock")
                    .unwrap_err();
            assert_eq!(
                error.to_string(),
                "cannot determine platform state directory for the run lock",
                "{os}"
            );
        }
    }

    #[test]
    fn lock_open_errors_preserve_context_and_existing_contents() {
        let root = tempfile::tempdir().unwrap();
        let repository = git2::Repository::init(root.path()).unwrap();
        let path = repository.commondir().join("dotfiles-run.lock");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("keep"), "preserve").unwrap();

        let error = RunLock::acquire(
            root.path(),
            &MapEnv::new(),
            Platform::new(Os::Windows, false),
            "install",
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("opening run lock {}", path.display())
        );
        assert!(error.downcast_ref::<std::io::Error>().is_some());
        assert_eq!(
            std::fs::read_to_string(path.join("keep")).unwrap(),
            "preserve"
        );
    }

    #[test]
    fn non_repository_locks_ignore_blank_and_relative_state_directories() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = std::path::absolute(fixture.path()).unwrap();
        let home = root.join("home");

        for (os, state_key, home_key) in [
            (Os::Linux, "XDG_STATE_HOME", "HOME"),
            (Os::Windows, "LOCALAPPDATA", "USERPROFILE"),
        ] {
            let platform = Platform::new(os, false);
            let fallback = MapEnv::new().with(home_key, &home);
            let expected = repository_state_path(&root, &fallback, platform, "lock").unwrap();
            for invalid in ["", "relative-state"] {
                let env = fallback.clone().with(state_key, invalid);
                let actual = repository_state_path(&root, &env, platform, "lock").unwrap();
                assert_eq!(actual, expected, "{os}: {state_key}={invalid:?}");
                assert!(actual.is_absolute(), "{os}: lock must not depend on cwd");
            }
        }
        assert!(!home.exists(), "path resolution must remain read-only");
    }

    #[test]
    fn non_repository_locks_reject_missing_absolute_home_and_state_directories() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = std::path::absolute(fixture.path()).unwrap();

        for (os, state_key, home_key) in [
            (Os::Linux, "XDG_STATE_HOME", "HOME"),
            (Os::Windows, "LOCALAPPDATA", "USERPROFILE"),
        ] {
            for invalid in ["", "relative-home"] {
                let env = MapEnv::new().with(state_key, "").with(home_key, invalid);
                let error = repository_state_path(&root, &env, Platform::new(os, false), "lock")
                    .unwrap_err();
                assert!(
                    error
                        .to_string()
                        .contains("cannot determine platform state directory"),
                    "{os}: {home_key}={invalid:?}: {error}"
                );
            }
        }
    }

    #[test]
    fn blank_state_override_and_home_fallback_contend_for_the_same_run_lock() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = std::path::absolute(fixture.path()).unwrap();
        for (os, state_key, home_key) in [
            (Os::Linux, "XDG_STATE_HOME", "HOME"),
            (Os::Windows, "LOCALAPPDATA", "USERPROFILE"),
        ] {
            let fallback = MapEnv::new().with(home_key, root.join(os.to_string()));
            let platform = Platform::new(os, false);
            let _holder = RunLock::acquire(&root, &fallback, platform, "install").unwrap();
            let expected =
                repository_state_path(&root, &fallback, platform, "dotfiles-run.lock").unwrap();
            let contender = fallback.with(state_key, "");
            assert_eq!(
                repository_state_path(&root, &contender, platform, "dotfiles-run.lock").unwrap(),
                expected,
                "{os}: resolve inside the fixture before attempting the contender"
            );
            let error = RunLock::acquire(&root, &contender, platform, "update").unwrap_err();
            assert!(
                error.to_string().contains("another dotfiles run"),
                "{os}: a blank override must not bypass an existing lock: {error}"
            );
        }
    }
}
