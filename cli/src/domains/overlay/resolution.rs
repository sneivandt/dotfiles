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
    parse_env_overlay(env.var_os("DOTFILES_OVERLAY"))
}

fn parse_env_overlay(raw: Option<std::ffi::OsString>) -> Option<PathBuf> {
    raw.filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// The git config key used to persist the overlay path.
const OVERLAY_KEY: &str = "dotfiles.overlay";

/// Try to read the persisted overlay path from the repository's local git
/// config (`dotfiles.overlay`).
///
/// # Errors
///
/// Returns an error if the repository or its local configuration cannot be read.
pub fn read_persisted(root: &Path) -> Result<Option<PathBuf>> {
    crate::infra::config::git_state::read_local(root, OVERLAY_KEY)
        .map(|value| value.map(PathBuf::from))
}

/// Persist the overlay path to the repository's local git config so future
/// runs use the same overlay without requiring the CLI flag.
///
/// # Errors
///
/// Returns an error if the repository cannot be discovered or the config
/// cannot be written, or the path cannot be stored without loss.
pub fn persist(root: &Path, overlay_path: &Path) -> Result<()> {
    crate::infra::config::git_state::persist_local(
        root,
        OVERLAY_KEY,
        overlay_path
            .to_str()
            .context("cannot save a non-UTF-8 overlay path")?,
    )
}

/// Where a startup overlay selection came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlaySource {
    Argument,
    Environment,
    Repository,
}

/// Resolve the absolute overlay path and its source without saving it.
/// Non-interactive runs decline explicit linked worktrees without prompting.
pub(crate) fn resolve_from_args(
    cli_overlay: Option<&Path>,
    root: &Path,
    env: &dyn crate::infra::env::Env,
    non_interactive: bool,
) -> Result<Option<(PathBuf, OverlaySource)>> {
    resolve_from_args_with_confirmation(
        cli_overlay,
        root,
        env,
        non_interactive,
        confirm_linked_worktree,
    )
}

/// Resolve an overlay for a read-only discovery command without persisting an
/// explicit path.
///
/// Explicit linked worktrees retain the normal confirmation requirement.
///
/// # Errors
///
/// Returns an error if saved configuration cannot be read, a linked worktree is
/// declined, its confirmation prompt fails, or the selected path cannot be made
/// absolute.
pub fn resolve_read_only(
    cli_overlay: Option<&Path>,
    root: &Path,
    env: &dyn crate::infra::env::Env,
) -> Result<Option<PathBuf>> {
    resolve_read_only_with_confirmation(cli_overlay, root, env, confirm_linked_worktree)
}

fn resolve_read_only_with_confirmation(
    cli_overlay: Option<&Path>,
    root: &Path,
    env: &dyn crate::infra::env::Env,
    confirm: impl FnOnce(&Path) -> Result<bool>,
) -> Result<Option<PathBuf>> {
    let non_interactive = env.var_os("CI").is_some();
    Ok(
        resolve_from_args_with_confirmation(cli_overlay, root, env, non_interactive, confirm)?
            .map(|(path, _)| path),
    )
}

fn resolve_with_confirmation(
    cli_overlay: Option<&Path>,
    root: &Path,
    env: &dyn crate::infra::env::Env,
    confirm: impl FnOnce(&Path) -> Result<bool>,
) -> Result<Option<(PathBuf, OverlaySource)>> {
    if let Some(path) = cli_overlay {
        let path = absolute_overlay_path(path)?;
        if is_linked_worktree(&path)? && !confirm(&path)? {
            bail!(
                "overlay path {} is a linked Git worktree; selection cancelled",
                path.display()
            );
        }
        return Ok(Some((path, OverlaySource::Argument)));
    }
    if let Some(path) = read_from_env(env) {
        return Ok(Some((
            absolute_overlay_path(&path)?,
            OverlaySource::Environment,
        )));
    }
    if crate::infra::provisioning::resuming_extensions(env)
        && env
            .var_os("DOTFILES_OVERLAY")
            .is_some_and(|path| path.is_empty())
    {
        // The marker explicitly recorded a run without an overlay.
        return Ok(None);
    }
    read_persisted(root)?
        .map(|path| Ok((absolute_overlay_path(&path)?, OverlaySource::Repository)))
        .transpose()
}

fn resolve_from_args_with_confirmation(
    cli_overlay: Option<&Path>,
    root: &Path,
    env: &dyn crate::infra::env::Env,
    non_interactive: bool,
    confirm: impl FnOnce(&Path) -> Result<bool>,
) -> Result<Option<(PathBuf, OverlaySource)>> {
    resolve_with_confirmation(cli_overlay, root, env, |path| {
        if non_interactive {
            Ok(false)
        } else {
            confirm(path)
        }
    })
}

fn absolute_overlay_path(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)
        .with_context(|| format!("resolving overlay path: {}", path.display()))?;
    let metadata = std::fs::metadata(&absolute)
        .with_context(|| format!("cannot read overlay directory {}", absolute.display()))?;
    anyhow::ensure!(
        metadata.is_dir(),
        "overlay path is not a directory: {}",
        absolute.display()
    );
    Ok(absolute)
}

fn is_linked_worktree(path: &Path) -> Result<bool> {
    let marker = path.join(".git");
    let Some(metadata) =
        crate::infra::fs::symlink_metadata_optional(&marker, "inspect overlay .git")?
    else {
        return Ok(false);
    };
    if metadata.is_symlink() {
        return Ok(std::fs::metadata(marker)
            .context("resolve overlay .git")?
            .is_file());
    }
    Ok(metadata.is_file())
}

fn prompt_on_terminal(
    input_terminal: bool,
    error_terminal: bool,
    prompt: impl FnOnce() -> Result<bool>,
) -> Result<bool> {
    if !input_terminal || !error_terminal {
        return Ok(false);
    }
    prompt()
}

fn confirm_linked_worktree(path: &Path) -> Result<bool> {
    prompt_on_terminal(
        io::stdin().is_terminal(),
        io::stderr().is_terminal(),
        || read_worktree_confirmation(path),
    )
}

#[allow(
    clippy::print_stderr,
    reason = "the interactive safety prompt must remain on one terminal line"
)]
fn read_worktree_confirmation(path: &Path) -> Result<bool> {
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
    fn worktree_prompt_requires_both_terminal_streams() {
        for (input, error) in [(false, false), (false, true), (true, false)] {
            assert!(!prompt_on_terminal(input, error, || panic!("cannot prompt")).unwrap());
        }
        for answer in [false, true] {
            assert_eq!(
                prompt_on_terminal(true, true, || Ok(answer)).unwrap(),
                answer
            );
        }
    }

    #[test]
    fn parse_env_overlay_returns_some_for_valid_path() {
        assert_eq!(
            parse_env_overlay(Some("/home/user/overlay".into())),
            Some(PathBuf::from("/home/user/overlay"))
        );
    }

    #[test]
    fn parse_env_overlay_returns_none_for_none() {
        assert_eq!(parse_env_overlay(None), None);
    }

    #[test]
    fn parse_env_overlay_returns_none_for_empty_string() {
        assert_eq!(parse_env_overlay(Some(std::ffi::OsString::new())), None);
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
        let result = read_persisted(&root).unwrap();
        assert_eq!(result, Some(overlay));
        drop(dir);
    }

    #[cfg(unix)]
    #[test]
    fn unrepresentable_overlay_does_not_replace_previous_selection() {
        use std::os::unix::ffi::OsStrExt as _;

        let (_dir, root) = init_test_repo();
        let previous = root.join("previous");
        let current = root.join(std::ffi::OsStr::from_bytes(b"selected-\xff"));
        let lossy = root.join("selected-\u{fffd}");
        for path in [&previous, &current, &lossy] {
            std::fs::create_dir(path).unwrap();
        }
        persist(&root, &previous).unwrap();
        let env = crate::infra::env::MapEnv::new();
        assert_eq!(
            resolve_from_args(Some(&current), &root, &env, true).unwrap(),
            Some((current.clone(), OverlaySource::Argument))
        );
        assert!(persist(&root, &current).is_err());
        assert_eq!(
            resolve_read_only(None, &root, &env).unwrap(),
            Some(previous)
        );
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_worktree_marker_does_not_bypass_confirmation() {
        let (_dir, root) = init_test_repo();
        let overlay = root.join("overlay");
        std::fs::create_dir(&overlay).unwrap();
        std::os::unix::fs::symlink(".git", overlay.join(".git")).unwrap();
        let env = crate::infra::env::MapEnv::new();
        assert!(resolve_read_only(Some(&overlay), &root, &env).is_err());
        assert!(resolve_from_args(Some(&overlay), &root, &env, true).is_err());
        assert_eq!(read_persisted(&root).unwrap(), None);
    }

    #[test]
    fn read_persisted_returns_none_when_unset() {
        let (dir, root) = init_test_repo();
        let result = read_persisted(&root).unwrap();
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
        let result = read_persisted(&root).unwrap();
        assert_eq!(result, Some(second));
        drop(dir);
    }

    #[test]
    fn read_persisted_reports_invalid_repository() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Prevent discovery of the enclosing checkout when fixtures live under it.
        std::fs::write(dir.path().join(".git"), "gitdir: missing-repository\n")
            .expect("write discovery boundary");
        assert!(git2::Repository::discover(dir.path()).is_err());
        assert!(read_persisted(dir.path()).is_err());
    }

    #[test]
    fn malformed_saved_overlay_is_reported_unless_explicitly_overridden() {
        use crate::infra::env::MapEnv;

        let (_dir, root) = init_test_repo();
        let config = root.join(".git/config");
        let malformed = "[dotfiles\noverlay = saved\n";
        std::fs::write(&config, malformed).unwrap();
        let overlay = tempfile::tempdir().unwrap();
        for read_only in [false, true] {
            for (cli, environment) in [(false, false), (true, false), (false, true), (true, true)] {
                let env = if environment {
                    MapEnv::new().with("DOTFILES_OVERLAY", overlay.path().to_str().unwrap())
                } else {
                    MapEnv::new()
                };
                let selected = cli.then_some(overlay.path());
                let result = if read_only {
                    resolve_read_only(selected, &root, &env)
                } else {
                    resolve_from_args(selected, &root, &env, true)
                        .map(|selection| selection.map(|(path, _)| path))
                };
                if cli || environment {
                    assert_eq!(result.unwrap().as_deref(), Some(overlay.path()));
                } else {
                    let error = result.unwrap_err();
                    assert!(error.downcast_ref::<git2::Error>().is_some(), "{error:#}");
                }
                assert_eq!(std::fs::read_to_string(&config).unwrap(), malformed);
            }
        }
    }

    #[test]
    fn resolve_from_args_prefers_cli_arg() {
        let (dir, root) = init_test_repo();
        let cli_path = root.join("cli-overlay");
        std::fs::create_dir(&cli_path).unwrap();
        let result = resolve_from_args(
            Some(&cli_path),
            &root,
            &crate::infra::env::MapEnv::new(),
            false,
        )
        .expect("ordinary overlay should resolve");
        assert_eq!(result, Some((cli_path, OverlaySource::Argument)));
        assert_eq!(read_persisted(&root).unwrap(), None);
        drop(dir);
    }

    #[test]
    fn resolve_read_only_declines_linked_worktree_in_ci_without_prompting() {
        let (_dir, root) = init_test_repo();
        let worktree = root.join("linked-worktree");
        std::fs::create_dir(&worktree).unwrap();
        std::fs::write(worktree.join(".git"), "gitdir: /repo/.git/worktrees/wt\n").unwrap();
        let ci_env = crate::infra::env::MapEnv::new().with("CI", "1");

        let error = resolve_read_only_with_confirmation(Some(&worktree), &root, &ci_env, |_| {
            panic!("CI must decline linked worktrees without prompting")
        })
        .unwrap_err();
        assert!(
            error.to_string().contains("linked Git worktree"),
            "{error:#}"
        );
    }

    #[test]
    fn resolve_from_args_returns_none_when_nothing_configured() {
        let (dir, root) = init_test_repo();
        let result = resolve_from_args(None, &root, &crate::infra::env::MapEnv::new(), false)
            .expect("missing overlay should resolve");
        assert_eq!(result, None);
        drop(dir);
    }

    #[test]
    fn read_only_resolution_does_not_persist_an_explicit_overlay() {
        let (dir, root) = init_test_repo();
        let overlay = root.join("discovery-overlay");
        std::fs::create_dir(&overlay).unwrap();

        let result = resolve_read_only(Some(&overlay), &root, &crate::infra::env::MapEnv::new())
            .expect("read-only overlay resolution");

        assert_eq!(result, Some(overlay));
        assert_eq!(read_persisted(&root).unwrap(), None);
        drop(dir);
    }

    #[test]
    fn an_empty_overlay_only_suppresses_saved_selection_on_resume() {
        use crate::infra::env::MapEnv;

        let (_dir, root) = init_test_repo();
        let overlay = root.join("saved-overlay");
        std::fs::create_dir(&overlay).unwrap();
        persist(&root, &overlay).unwrap();
        for (resuming, explicit_empty) in
            [(false, false), (false, true), (true, false), (true, true)]
        {
            let mut env = MapEnv::new();
            if resuming {
                env = env.with("DOTFILES_RESUME_VSCODE_EXTENSIONS", "1");
            }
            if explicit_empty {
                env = env.with("DOTFILES_OVERLAY", "");
            }
            let expected = (!(resuming && explicit_empty)).then_some(overlay.as_path());
            assert_eq!(
                resolve_read_only(None, &root, &env).unwrap().as_deref(),
                expected
            );
        }
    }

    #[test]
    fn resolve_from_args_falls_back_to_persisted() {
        let (dir, root) = init_test_repo();
        let overlay = root.join("persisted-overlay");
        std::fs::create_dir(&overlay).unwrap();
        persist(&root, &overlay).expect("persist");
        let result = resolve_from_args(None, &root, &crate::infra::env::MapEnv::new(), false)
            .expect("persisted overlay should resolve");
        assert_eq!(result, Some((overlay, OverlaySource::Repository)));
        drop(dir);
    }

    #[test]
    fn relative_cli_overlay_is_resolved_to_an_absolute_path() {
        let repo = tempfile::tempdir_in(".").expect("fixture repository");
        git2::Repository::init(repo.path()).expect("git init");
        let path = repo.path().join("relative-overlay");
        std::fs::create_dir(&path).unwrap();
        let expected = std::path::absolute(&path).expect("absolute overlay path");

        let selected = resolve_from_args(
            Some(&path),
            repo.path(),
            &crate::infra::env::MapEnv::new(),
            false,
        )
        .expect("resolve relative overlay");

        assert_eq!(selected, Some((expected, OverlaySource::Argument)));
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
        let (selected, _) = resolve_from_args(
            Some(&overlay),
            repo.path(),
            &crate::infra::env::MapEnv::new(),
            false,
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
            check_remove: false,
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
        let env_path = repo.path().join("env-overlay");
        let saved_path = repo.path().join("persisted-overlay");
        let cli_path = repo.path().join("cli-overlay");
        for path in [&env_path, &saved_path, &cli_path] {
            std::fs::create_dir(path).unwrap();
        }
        let env = crate::infra::env::MapEnv::new().with("DOTFILES_OVERLAY", &env_path);
        persist(repo.path(), &saved_path).expect("persist legacy path");

        for read_only in [false, true] {
            let resolver = |environment: &dyn crate::infra::env::Env| {
                if read_only {
                    resolve_read_only(None, repo.path(), environment)
                } else {
                    resolve_from_args(None, repo.path(), environment, false)
                        .map(|selection| selection.map(|(path, _)| path))
                }
            };
            let expected_env = std::path::absolute(&env_path).expect("absolute environment path");
            assert_eq!(
                resolver(&env).unwrap(),
                Some(expected_env),
                "environment must override persisted selection"
            );
            let expected_persisted = std::path::absolute(&saved_path).expect("absolute saved path");
            assert_eq!(
                resolver(&crate::infra::env::MapEnv::new()).unwrap(),
                Some(expected_persisted),
                "legacy relative persisted selections must be usable by subprocesses"
            );
        }

        let expected_cli = std::path::absolute(&cli_path).expect("absolute CLI path");
        assert_eq!(
            resolve_read_only(Some(&cli_path), repo.path(), &env).unwrap(),
            Some(expected_cli),
            "explicit selection must override the environment"
        );
        assert_eq!(
            read_persisted(repo.path()).unwrap(),
            Some(saved_path),
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
            false,
            |_| Ok(false),
        )
        .expect_err("declined linked worktree should be rejected");

        assert!(
            error.to_string().contains("selection cancelled"),
            "declined selection should explain why it stopped: {error}"
        );
        assert_eq!(
            read_persisted(&root).unwrap(),
            None,
            "declined linked worktree must not be persisted"
        );
        drop(dir);
    }

    #[test]
    fn linked_worktree_overlay_resolves_after_confirmation() {
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
            false,
            |_| Ok(true),
        )
        .expect("confirmed linked worktree should resolve");

        assert_eq!(
            result,
            Some((overlay.path().to_path_buf(), OverlaySource::Argument))
        );
        assert_eq!(read_persisted(&root).unwrap(), None);
        drop(dir);
    }

    #[test]
    fn non_interactive_linked_worktree_selection_never_prompts_or_persists() {
        let (_dir, root) = init_test_repo();
        let overlay = tempfile::tempdir().unwrap();
        std::fs::write(
            overlay.path().join(".git"),
            "gitdir: ../main/.git/worktrees/overlay\n",
        )
        .unwrap();
        let saved = root.join("previous-overlay");
        persist(&root, &saved).unwrap();

        let error = resolve_from_args_with_confirmation(
            Some(overlay.path()),
            &root,
            &crate::infra::env::MapEnv::new(),
            true,
            |_| panic!("non-interactive policy must bypass terminal confirmation"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("selection cancelled"));
        assert_eq!(read_persisted(&root).unwrap(), Some(saved));
    }

    #[test]
    fn non_interactive_primary_checkout_selection_resolves() {
        let (_dir, root) = init_test_repo();
        let overlay = tempfile::tempdir().unwrap();
        std::fs::create_dir(overlay.path().join(".git")).unwrap();

        let selected = resolve_from_args(
            Some(overlay.path()),
            &root,
            &crate::infra::env::MapEnv::new(),
            true,
        )
        .unwrap();

        assert_eq!(
            selected,
            Some((overlay.path().to_path_buf(), OverlaySource::Argument))
        );
    }

    #[test]
    fn git_directory_is_not_treated_as_a_linked_worktree() {
        let overlay = tempfile::tempdir().expect("overlay tempdir");
        std::fs::create_dir(overlay.path().join(".git")).expect("create git directory");

        assert!(
            !is_linked_worktree(overlay.path()).unwrap(),
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
