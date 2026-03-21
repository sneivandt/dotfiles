//! Private overlay repository resolution and persistence.
//!
//! Overlay repositories contain additional TOML configuration files and
//! custom scripts that extend the main dotfiles configuration.  The overlay
//! path is resolved from CLI args, the `DOTFILES_OVERLAY` environment
//! variable, or the repository's local git config (`dotfiles.overlay`).
use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

/// Try to read the overlay path from the `DOTFILES_OVERLAY` environment variable.
#[must_use]
pub fn read_from_env() -> Option<PathBuf> {
    parse_env_overlay(std::env::var("DOTFILES_OVERLAY").ok())
}

fn parse_env_overlay(raw: Option<String>) -> Option<PathBuf> {
    raw.filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Try to read the persisted overlay path from the repository's local git
/// config (`dotfiles.overlay`).
#[must_use]
pub fn read_persisted(root: &Path) -> Option<PathBuf> {
    let repo = git2::Repository::discover(root).ok()?;
    let config = repo.config().ok()?;
    let local = config.open_level(git2::ConfigLevel::Local).ok()?;
    local
        .get_string("dotfiles.overlay")
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Persist the overlay path to the repository's local git config so future
/// runs use the same overlay without requiring the CLI flag.
///
/// # Errors
///
/// Returns an error if the repository cannot be discovered or the config
/// cannot be written.
pub fn persist(root: &Path, overlay_path: &Path) -> Result<()> {
    let repo = git2::Repository::discover(root).context("finding git repository")?;
    let config = repo.config().context("opening git config")?;
    let mut local = config
        .open_level(git2::ConfigLevel::Local)
        .context("opening local git config")?;
    local
        .set_str("dotfiles.overlay", &overlay_path.display().to_string())
        .context("persisting overlay path to git config")
}

/// Resolve the overlay path from CLI arg, `DOTFILES_OVERLAY` env var, or
/// persisted git config.
///
/// When the overlay path is obtained from a CLI argument, it is persisted
/// to the repository's local git config so future runs use it automatically.
///
/// Returns `None` if no overlay is configured.
#[must_use]
#[allow(clippy::print_stderr)]
pub fn resolve_from_args(cli_overlay: Option<&Path>, root: &Path) -> Option<PathBuf> {
    if let Some(path) = cli_overlay {
        let path = path.to_path_buf();
        if let Err(e) = persist(root, &path) {
            eprintln!("warning: could not persist overlay path to git config: {e}");
        }
        return Some(path);
    }

    if let Some(path) = read_from_env() {
        return Some(path);
    }

    read_persisted(root)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn parse_env_overlay_returns_some_for_valid_path() {
        assert_eq!(
            parse_env_overlay(Some("/home/user/overlay".to_string())),
            Some(PathBuf::from("/home/user/overlay"))
        );
    }

    #[test]
    fn parse_env_overlay_returns_none_for_none() {
        assert_eq!(parse_env_overlay(None), None);
    }

    #[test]
    fn parse_env_overlay_returns_none_for_empty_string() {
        assert_eq!(parse_env_overlay(Some(String::new())), None);
    }

    fn init_test_repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = git2::Repository::init(dir.path()).expect("git init");
        let root = repo.workdir().expect("workdir").to_path_buf();
        (dir, root)
    }

    #[test]
    fn persist_and_read_persisted_round_trip() {
        let (dir, root) = init_test_repo();
        let overlay = PathBuf::from("/home/user/dotfiles-msft");
        persist(&root, &overlay).expect("persist should succeed");
        let result = read_persisted(&root);
        assert_eq!(result, Some(overlay));
        drop(dir);
    }

    #[test]
    fn read_persisted_returns_none_when_unset() {
        let (dir, root) = init_test_repo();
        let result = read_persisted(&root);
        assert_eq!(result, None);
        drop(dir);
    }

    #[test]
    fn persist_overwrites_previous_value() {
        let (dir, root) = init_test_repo();
        let first = PathBuf::from("/first/path");
        let second = PathBuf::from("/second/path");
        persist(&root, &first).expect("first persist");
        persist(&root, &second).expect("second persist");
        let result = read_persisted(&root);
        assert_eq!(result, Some(second));
        drop(dir);
    }

    #[test]
    fn read_persisted_returns_none_outside_git_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let result = read_persisted(dir.path());
        assert_eq!(result, None);
    }

    #[test]
    fn resolve_from_args_prefers_cli_arg() {
        let (dir, root) = init_test_repo();
        let cli_path = PathBuf::from("/cli/overlay");
        let result = resolve_from_args(Some(&cli_path), &root);
        assert_eq!(result, Some(cli_path.clone()));
        // Also persisted
        assert_eq!(read_persisted(&root), Some(cli_path));
        drop(dir);
    }

    #[test]
    fn resolve_from_args_returns_none_when_nothing_configured() {
        let (dir, root) = init_test_repo();
        let result = resolve_from_args(None, &root);
        assert_eq!(result, None);
        drop(dir);
    }

    #[test]
    fn resolve_from_args_falls_back_to_persisted() {
        let (dir, root) = init_test_repo();
        let overlay = PathBuf::from("/persisted/overlay");
        persist(&root, &overlay).expect("persist");
        let result = resolve_from_args(None, &root);
        assert_eq!(result, Some(overlay));
        drop(dir);
    }
}
