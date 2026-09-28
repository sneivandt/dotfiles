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

        if let Err(error) = file.try_lock() {
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
        drop(self.file.unlock());
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
    use std::io::Read as _;

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
    fn non_repository_state_paths_use_platform_precedence_and_repository_identity() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        for path in [&first, &second] {
            std::fs::create_dir(path).unwrap();
            // Stop discovery at the fixture even when TMPDIR is inside a checkout.
            std::fs::write(path.join(".git"), "not a git directory").unwrap();
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
}
