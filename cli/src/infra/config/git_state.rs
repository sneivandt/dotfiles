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
/// Returns `None` when no repository or key exists, or the stored value is empty.
///
/// # Errors
///
/// Returns an error when repository discovery or configuration reading fails.
pub fn read_local(root: &Path, key: &str) -> Result<Option<String>> {
    let repo = match git2::Repository::discover(root) {
        Ok(repo) => repo,
        Err(error)
            if error.code() == git2::ErrorCode::NotFound
                && error.class() == git2::ErrorClass::Repository =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error).context("finding git repository"),
    };
    let config = repo.config().context("opening git config")?;
    let local = config
        .open_level(git2::ConfigLevel::Local)
        .context("opening local git config")?;
    match local.get_string(key) {
        Ok(value) => Ok((!value.is_empty()).then_some(value)),
        Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {key} from git config")),
    }
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
            read_local(&root, "dotfiles.example").unwrap(),
            Some("value".to_string())
        );
        drop(dir);
    }

    #[test]
    fn read_returns_none_when_unset() {
        let (dir, root) = init_test_repo();
        assert_eq!(read_local(&root, "dotfiles.missing").unwrap(), None);
        drop(dir);
    }

    #[test]
    fn read_reports_invalid_repository() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(".git"), "not a git directory").unwrap();
        assert!(read_local(dir.path(), "dotfiles.example").is_err());
        let error = persist_local(dir.path(), "dotfiles.example", "value").unwrap_err();
        assert_eq!(error.to_string(), "finding git repository");
        assert!(error.downcast_ref::<git2::Error>().is_some());
    }

    #[test]
    fn read_distinguishes_missing_repository_from_malformed_config() {
        let absent = tempfile::tempdir().unwrap();
        assert_eq!(read_local(absent.path(), "dotfiles.example").unwrap(), None);
        assert!(read_local(&absent.path().join("missing"), "dotfiles.example").is_err());

        let (_dir, root) = init_test_repo();
        let config = root.join(".git/config");
        let malformed = "[dotfiles\nexample = saved\n";
        std::fs::write(&config, malformed).unwrap();
        let error = read_local(&root, "dotfiles.example").unwrap_err();
        assert!(error.downcast_ref::<git2::Error>().is_some(), "{error:#}");
        assert_eq!(std::fs::read_to_string(config).unwrap(), malformed);
    }

    #[test]
    fn persist_overwrites_previous_value() {
        let (dir, root) = init_test_repo();
        persist_local(&root, "dotfiles.example", "first").expect("first");
        persist_local(&root, "dotfiles.example", "second").expect("second");
        assert_eq!(
            read_local(&root, "dotfiles.example").unwrap(),
            Some("second".to_string())
        );
        drop(dir);
    }

    #[test]
    fn empty_values_are_unset_but_whitespace_and_unicode_are_preserved() {
        let (_dir, root) = init_test_repo();
        let nested = root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        for value in ["", " ", "日本語 path with spaces"] {
            persist_local(&nested, "dotfiles.example", value).unwrap();
            assert_eq!(
                read_local(&root, "dotfiles.example").unwrap(),
                (!value.is_empty()).then(|| value.to_string()),
                "{value:?}"
            );
        }
    }

    #[test]
    fn failed_persistence_preserves_existing_config_and_reports_the_key() {
        let (_dir, root) = init_test_repo();
        persist_local(&root, "dotfiles.example", "original").unwrap();
        let repo = git2::Repository::open(&root).unwrap();
        let config = repo.path().join("config");
        let lock = repo.path().join("config.lock");
        let before = std::fs::read(&config).unwrap();
        std::fs::write(&lock, "other writer").unwrap();

        let error = persist_local(&root, "dotfiles.example", "replacement").unwrap_err();

        assert_eq!(
            error.to_string(),
            "persisting dotfiles.example to git config"
        );
        assert!(error.downcast_ref::<git2::Error>().is_some());
        assert_eq!(std::fs::read(config).unwrap(), before);
        assert_eq!(std::fs::read_to_string(lock).unwrap(), "other writer");
        assert_eq!(
            read_local(&root, "dotfiles.example").unwrap().as_deref(),
            Some("original")
        );
    }
}
