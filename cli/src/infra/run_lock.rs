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
        if self.checkout_path.as_ref() == Some(&checkout_lock) {
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
            // On Linux, `flock(2)` exclusive-to-shared conversion is not atomic
            // (see `flock(2)` NOTES): another waiter can acquire the lock during
            // conversion, and `take_shared` surfaces that contention safely if a
            // legacy contender wins. Windows requires releasing the exclusive
            // lock first.
            #[cfg(windows)]
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
mod tests;
