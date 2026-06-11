//! Shared helpers for reading and persisting dotfiles state in the
//! repository's local git config.
//!
//! Both the profile (`dotfiles.profile`) and overlay (`dotfiles.overlay`)
//! settings persist a single string value to the repository's local git
//! config so future runs reuse the same selection without re-prompting or
//! requiring a CLI flag.  These functions centralise that read/write logic.
use anyhow::{Context as _, Result};
use std::path::Path;

/// Read a non-empty string value from the repository's local git config.
///
/// Returns `None` if the repository cannot be discovered, the local config
/// cannot be opened, the key is unset, or the stored value is empty.
#[must_use]
pub fn read_local(root: &Path, key: &str) -> Option<String> {
    let repo = git2::Repository::discover(root).ok()?;
    let config = repo.config().ok()?;
    let local = config.open_level(git2::ConfigLevel::Local).ok()?;
    local.get_string(key).ok().filter(|value| !value.is_empty())
}

/// Persist a string value to the repository's local git config.
///
/// # Errors
///
/// Returns an error if the repository cannot be discovered or the config
/// cannot be written.
pub fn persist_local(root: &Path, key: &str, value: &str) -> Result<()> {
    let repo = git2::Repository::discover(root).context("finding git repository")?;
    let config = repo.config().context("opening git config")?;
    let mut local = config
        .open_level(git2::ConfigLevel::Local)
        .context("opening local git config")?;
    local
        .set_str(key, value)
        .with_context(|| format!("persisting {key} to git config"))
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "test code uses panicking helpers")]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn init_test_repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = git2::Repository::init(dir.path()).expect("git init");
        let root = repo.workdir().expect("workdir").to_path_buf();
        (dir, root)
    }

    #[test]
    fn persist_and_read_round_trip() {
        let (dir, root) = init_test_repo();
        persist_local(&root, "dotfiles.example", "value").expect("persist");
        assert_eq!(
            read_local(&root, "dotfiles.example"),
            Some("value".to_string())
        );
        drop(dir);
    }

    #[test]
    fn read_returns_none_when_unset() {
        let (dir, root) = init_test_repo();
        assert_eq!(read_local(&root, "dotfiles.missing"), None);
        drop(dir);
    }

    #[test]
    fn read_returns_none_outside_git_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(read_local(dir.path(), "dotfiles.example"), None);
    }

    #[test]
    fn persist_overwrites_previous_value() {
        let (dir, root) = init_test_repo();
        persist_local(&root, "dotfiles.example", "first").expect("first");
        persist_local(&root, "dotfiles.example", "second").expect("second");
        assert_eq!(
            read_local(&root, "dotfiles.example"),
            Some("second".to_string())
        );
        drop(dir);
    }
}
