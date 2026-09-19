//! Private overlay repository resolution and persistence.
//!
//! Overlay repositories contain additional TOML configuration files and
//! custom scripts that extend the main dotfiles configuration.  The overlay
//! path is resolved from CLI args, the `DOTFILES_OVERLAY` environment
//! variable, or the repository's local git config (`dotfiles.overlay`).
use anyhow::{Context as _, Result, bail};
use std::io::{self, IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};

/// Try to read the overlay path from the `DOTFILES_OVERLAY` environment variable.
#[must_use]
pub fn read_from_env(env: &dyn crate::infra::env::Env) -> Option<PathBuf> {
    parse_env_overlay(env.var("DOTFILES_OVERLAY"))
}

fn parse_env_overlay(raw: Option<String>) -> Option<PathBuf> {
    raw.filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// The git config key used to persist the overlay path.
const OVERLAY_KEY: &str = "dotfiles.overlay";

/// Try to read the persisted overlay path from the repository's local git
/// config (`dotfiles.overlay`).
#[must_use]
pub fn read_persisted(root: &Path) -> Option<PathBuf> {
    crate::infra::config::git_state::read_local(root, OVERLAY_KEY).map(PathBuf::from)
}

/// Persist the overlay path to the repository's local git config so future
/// runs use the same overlay without requiring the CLI flag.
///
/// # Errors
///
/// Returns an error if the repository cannot be discovered or the config
/// cannot be written.
pub fn persist(root: &Path, overlay_path: &Path) -> Result<()> {
    crate::infra::config::git_state::persist_local(
        root,
        OVERLAY_KEY,
        &overlay_path.display().to_string(),
    )
}

/// Resolve the overlay path from CLI arg, `DOTFILES_OVERLAY` env var, or
/// persisted git config.
///
/// When the overlay path is obtained from a CLI argument, it is persisted
/// to the repository's local git config so future runs use it automatically.
/// Relative selections are made absolute against the invocation directory
/// before use or persistence.
///
/// Returns `None` if no overlay is configured.
///
/// # Errors
///
/// Returns an error if a linked Git worktree is declined as the overlay or its
/// confirmation prompt cannot be read, or the selected path cannot be made
/// absolute.
pub fn resolve_from_args(
    cli_overlay: Option<&Path>,
    root: &Path,
    env: &dyn crate::infra::env::Env,
) -> Result<Option<PathBuf>> {
    resolve_from_args_with_confirmation(cli_overlay, root, env, confirm_linked_worktree)
}

/// Resolve an overlay for a read-only discovery command without persisting an
/// explicit path.
///
/// Explicit linked worktrees retain the normal confirmation requirement.
///
/// # Errors
///
/// Returns an error if a linked worktree is declined or its confirmation
/// prompt cannot be read, or the selected path cannot be made absolute.
pub fn resolve_read_only(
    cli_overlay: Option<&Path>,
    root: &Path,
    env: &dyn crate::infra::env::Env,
) -> Result<Option<PathBuf>> {
    if let Some(path) = cli_overlay {
        let path = absolute_overlay_path(path)?;
        if is_linked_worktree(&path) && !confirm_linked_worktree(&path)? {
            bail!(
                "overlay path {} is a linked Git worktree; selection cancelled",
                path.display()
            );
        }
        return Ok(Some(path));
    }
    read_from_env(env)
        .or_else(|| read_persisted(root))
        .as_deref()
        .map(absolute_overlay_path)
        .transpose()
}

#[allow(
    clippy::print_stderr,
    reason = "overlay persistence failures are intentionally surfaced before logger setup completes"
)]
fn resolve_from_args_with_confirmation(
    cli_overlay: Option<&Path>,
    root: &Path,
    env: &dyn crate::infra::env::Env,
    confirm: impl FnOnce(&Path) -> Result<bool>,
) -> Result<Option<PathBuf>> {
    if let Some(path) = cli_overlay {
        let path = absolute_overlay_path(path)?;
        if is_linked_worktree(&path) && !confirm(&path)? {
            bail!(
                "overlay path {} is a linked Git worktree; selection cancelled",
                path.display()
            );
        }
        if let Err(e) = persist(root, &path) {
            eprintln!("warning: could not persist overlay path to git config: {e}");
        }
        return Ok(Some(path));
    }

    read_from_env(env)
        .or_else(|| read_persisted(root))
        .as_deref()
        .map(absolute_overlay_path)
        .transpose()
}

fn absolute_overlay_path(path: &Path) -> Result<PathBuf> {
    std::path::absolute(path).with_context(|| format!("resolving overlay path: {}", path.display()))
}

fn is_linked_worktree(path: &Path) -> bool {
    path.join(".git").is_file()
}

#[allow(
    clippy::print_stderr,
    reason = "the interactive safety prompt must remain on one terminal line"
)]
fn confirm_linked_worktree(path: &Path) -> Result<bool> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Ok(false);
    }

    eprint!(
        "Overlay path {} is a linked Git worktree. Use this worktree anyway? [y/N] ",
        path.display()
    );
    io::stderr()
        .flush()
        .context("flushing overlay worktree confirmation")?;

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context("reading overlay worktree confirmation")?;
    Ok(is_affirmative(&answer))
}

fn is_affirmative(answer: &str) -> bool {
    let answer = answer.trim();
    answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes")
}

#[cfg(test)]
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
        let overlay = PathBuf::from("/home/user/dotfiles-overlay");
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
        let cli_path = std::path::absolute("cli-overlay").expect("absolute CLI path");
        let result = resolve_from_args(Some(&cli_path), &root, &crate::infra::env::MapEnv::new())
            .expect("ordinary overlay should resolve");
        assert_eq!(result, Some(cli_path.clone()));
        // Also persisted
        assert_eq!(read_persisted(&root), Some(cli_path));
        drop(dir);
    }

    #[test]
    fn resolve_from_args_returns_none_when_nothing_configured() {
        let (dir, root) = init_test_repo();
        let result = resolve_from_args(None, &root, &crate::infra::env::MapEnv::new())
            .expect("missing overlay should resolve");
        assert_eq!(result, None);
        drop(dir);
    }

    #[test]
    fn read_only_resolution_does_not_persist_an_explicit_overlay() {
        let (dir, root) = init_test_repo();
        let overlay = std::path::absolute("discovery-overlay").expect("absolute overlay path");

        let result = resolve_read_only(Some(&overlay), &root, &crate::infra::env::MapEnv::new())
            .expect("read-only overlay resolution");

        assert_eq!(result, Some(overlay));
        assert_eq!(read_persisted(&root), None);
        drop(dir);
    }

    #[test]
    fn resolve_from_args_falls_back_to_persisted() {
        let (dir, root) = init_test_repo();
        let overlay = std::path::absolute("persisted-overlay").expect("absolute overlay path");
        persist(&root, &overlay).expect("persist");
        let result = resolve_from_args(None, &root, &crate::infra::env::MapEnv::new())
            .expect("persisted overlay should resolve");
        assert_eq!(result, Some(overlay));
        drop(dir);
    }

    #[test]
    fn relative_cli_overlay_is_absolute_before_use_and_persistence() {
        let repo = tempfile::tempdir_in(".").expect("fixture repository");
        git2::Repository::init(repo.path()).expect("git init");
        let path = Path::new("relative-overlay");
        let expected = std::path::absolute(path).expect("absolute overlay path");

        let selected =
            resolve_from_args(Some(path), repo.path(), &crate::infra::env::MapEnv::new())
                .expect("resolve relative overlay");

        assert_eq!(selected, Some(expected.clone()));
        assert_eq!(read_persisted(repo.path()), Some(expected));
    }

    #[test]
    fn relative_overlay_scripts_are_not_resolved_twice_after_changing_child_directory() {
        use crate::domains::overlay::config::scripts::ScriptEntry;
        use crate::domains::overlay::resources::script::ScriptResource;
        use crate::engine::{IntrinsicState as _, RemovableResource as _, Resource as _};
        use crate::infra::exec::{ExecResult, MockExecutor};

        let repo = tempfile::tempdir_in(".").expect("fixture repository");
        git2::Repository::init(repo.path()).expect("git init");
        let overlay = repo.path().join("overlay");
        std::fs::create_dir(&overlay).expect("fixture overlay");
        std::fs::write(overlay.join("script.sh"), "#!/bin/sh\n").expect("fixture script");
        let selected = resolve_from_args(
            Some(&overlay),
            repo.path(),
            &crate::infra::env::MapEnv::new(),
        )
        .unwrap()
        .expect("selected overlay");
        let expected_script = std::path::absolute(overlay.join("script.sh")).unwrap();
        let expected_root = std::path::absolute(&overlay).unwrap();
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .times(4)
            .withf(move |spec| {
                spec.program() == "sh"
                    && spec.working_dir() == Some(expected_root.as_path())
                    && spec
                        .arguments()
                        .first()
                        .is_some_and(|argument| argument == expected_script.as_os_str())
            })
            .returning(|_| Ok(ExecResult::success("")));
        let entry = ScriptEntry {
            name: "Fixture".to_string(),
            path: "script.sh".to_string(),
            description: None,
        };
        let resource = ScriptResource::from_entry(&entry, &selected, std::sync::Arc::new(mock))
            .expect("script resource");

        assert_eq!(
            resource.current_state().unwrap(),
            crate::engine::ResourceState::Correct
        );
        resource.apply().unwrap();
        resource.preview_with_output().unwrap();
        resource.remove().unwrap();
    }

    #[test]
    fn all_resolvers_anchor_relative_selections_without_changing_precedence() {
        let repo = tempfile::tempdir_in(".").expect("fixture repository");
        git2::Repository::init(repo.path()).expect("git init");
        let env = crate::infra::env::MapEnv::new().with("DOTFILES_OVERLAY", "env-overlay");
        persist(repo.path(), Path::new("persisted-overlay")).expect("persist legacy path");

        for resolver in [resolve_from_args, resolve_read_only] {
            let expected_env =
                std::path::absolute("env-overlay").expect("absolute environment path");
            assert_eq!(
                resolver(None, repo.path(), &env).unwrap(),
                Some(expected_env),
                "environment must override persisted selection"
            );
            let expected_persisted =
                std::path::absolute("persisted-overlay").expect("absolute saved path");
            assert_eq!(
                resolver(None, repo.path(), &crate::infra::env::MapEnv::new()).unwrap(),
                Some(expected_persisted),
                "legacy relative persisted selections must be usable by subprocesses"
            );
        }

        let expected_cli = std::path::absolute("cli-overlay").expect("absolute CLI path");
        assert_eq!(
            resolve_read_only(Some(Path::new("cli-overlay")), repo.path(), &env).unwrap(),
            Some(expected_cli),
            "explicit selection must override the environment"
        );
        assert_eq!(
            read_persisted(repo.path()),
            Some(PathBuf::from("persisted-overlay")),
            "read-only discovery must not rewrite persisted state"
        );
    }

    #[test]
    fn linked_worktree_overlay_requires_confirmation_before_persisting() {
        let (dir, root) = init_test_repo();
        let overlay = tempfile::tempdir().expect("overlay tempdir");
        std::fs::write(
            overlay.path().join(".git"),
            "gitdir: ../main/.git/worktrees/overlay\n",
        )
        .expect("write linked worktree marker");

        let error = resolve_from_args_with_confirmation(
            Some(overlay.path()),
            &root,
            &crate::infra::env::MapEnv::new(),
            |_| Ok(false),
        )
        .expect_err("declined linked worktree should be rejected");

        assert!(
            error.to_string().contains("selection cancelled"),
            "declined selection should explain why it stopped: {error}"
        );
        assert_eq!(
            read_persisted(&root),
            None,
            "declined linked worktree must not be persisted"
        );
        drop(dir);
    }

    #[test]
    fn linked_worktree_overlay_is_persisted_after_confirmation() {
        let (dir, root) = init_test_repo();
        let overlay = tempfile::tempdir().expect("overlay tempdir");
        std::fs::write(
            overlay.path().join(".git"),
            "gitdir: ../main/.git/worktrees/overlay\n",
        )
        .expect("write linked worktree marker");

        let result = resolve_from_args_with_confirmation(
            Some(overlay.path()),
            &root,
            &crate::infra::env::MapEnv::new(),
            |_| Ok(true),
        )
        .expect("confirmed linked worktree should resolve");

        assert_eq!(result.as_deref(), Some(overlay.path()));
        assert_eq!(read_persisted(&root).as_deref(), Some(overlay.path()));
        drop(dir);
    }

    #[test]
    fn git_directory_is_not_treated_as_a_linked_worktree() {
        let overlay = tempfile::tempdir().expect("overlay tempdir");
        std::fs::create_dir(overlay.path().join(".git")).expect("create git directory");

        assert!(
            !is_linked_worktree(overlay.path()),
            "a primary checkout should not require linked-worktree confirmation"
        );
    }

    #[test]
    fn worktree_confirmation_defaults_to_no() {
        for answer in ["", "\n", "n", "no", "unexpected"] {
            assert!(
                !is_affirmative(answer),
                "{answer:?} should decline worktree use"
            );
        }
        for answer in ["y", "Y", "yes", "YES"] {
            assert!(
                is_affirmative(answer),
                "{answer:?} should confirm worktree use"
            );
        }
    }
}
