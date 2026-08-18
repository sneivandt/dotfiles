//! Cross-process serialization for repository runs.

use std::fs::{File, OpenOptions};
use std::io::{Seek as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail};
use base64::Engine as _;
use serde::Serialize;
use sha2::{Digest as _, Sha256};

use crate::infra::env::Env;
use crate::infra::platform::Platform;

/// An exclusive lock held for the lifetime of one command run.
#[derive(Debug)]
pub(crate) struct RunLock {
    file: File,
}

#[derive(Serialize)]
struct Owner<'a> {
    pid: u32,
    started_unix_seconds: u64,
    command: &'a str,
}

impl RunLock {
    /// Acquire the lock associated with `root`.
    pub(crate) fn acquire(
        root: &Path,
        env: &dyn Env,
        platform: Platform,
        command: &str,
    ) -> Result<Self> {
        let path = repository_state_path(root, env, platform, "dotfiles-run.lock")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating run-lock directory {}", parent.display()))?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("opening run lock {}", path.display()))?;

        if let Err(error) = fs2::FileExt::try_lock_exclusive(&file) {
            let owner = std::fs::read_to_string(&path).unwrap_or_default();
            let owner = owner.trim();
            if owner.is_empty() {
                bail!(
                    "another dotfiles run holds {}; wait for it to finish and retry ({error})",
                    path.display()
                );
            }
            bail!(
                "another dotfiles run holds {}: {owner}; wait for it to finish and retry",
                path.display()
            );
        }

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

        Ok(Self { file })
    }
}

impl Drop for RunLock {
    fn drop(&mut self) {
        drop(fs2::FileExt::unlock(&self.file));
    }
}

/// Resolve a repository-scoped internal state path.
pub(crate) fn repository_state_path(
    root: &Path,
    env: &dyn Env,
    platform: Platform,
    filename: &str,
) -> Result<PathBuf> {
    if let Ok(repository) = git2::Repository::discover(root) {
        return Ok(repository.commondir().join(filename));
    }

    let state_root = if platform.is_windows() {
        env.var_os("LOCALAPPDATA").map(PathBuf::from).or_else(|| {
            env.var_os("USERPROFILE")
                .map(PathBuf::from)
                .map(|home| home.join("AppData").join("Local"))
        })
    } else {
        env.var_os("XDG_STATE_HOME").map(PathBuf::from).or_else(|| {
            env.var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local").join("state"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::env::MapEnv;
    use crate::infra::platform::{Os, Platform};

    #[test]
    fn prevents_overlapping_runs_and_releases_on_drop() {
        let root = tempfile::tempdir().expect("tempdir");
        git2::Repository::init(root.path()).expect("init repository");
        let env = MapEnv::new();

        let platform = Platform::new(Os::Windows, false);
        let first = RunLock::acquire(root.path(), &env, platform, "install").expect("first lock");
        let error =
            RunLock::acquire(root.path(), &env, platform, "update").expect_err("second lock");
        assert!(
            error.to_string().contains("another dotfiles run"),
            "unexpected error: {error:#}"
        );
        drop(first);

        RunLock::acquire(root.path(), &env, platform, "update").expect("lock after release");
    }
}
