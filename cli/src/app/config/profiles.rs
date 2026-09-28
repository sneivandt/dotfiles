//! Profile selection and resolution.

mod environment;
mod persistence;
mod prompt;
mod resolution;

use anyhow::Result;
use std::path::Path;

use crate::infra::config::category_matcher::Category;
use crate::infra::platform::Platform;

pub use environment::read_from_env;
pub use persistence::{persist, read_persisted};
pub use prompt::prompt_interactive;
pub use resolution::Profile;
pub use resolution::resolve;

fn selected_profile_name(
    cli_profile: Option<&str>,
    root: &Path,
    env: &dyn crate::infra::env::Env,
) -> Option<String> {
    cli_profile
        .map(str::to_owned)
        .or_else(|| read_from_env(env))
        .or_else(|| read_persisted(root))
}

/// One built-in role profile used by selection prompts and completions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileInfo {
    /// Profile name used with `--profile`.
    pub name: &'static str,
    /// User-facing description.
    pub description: &'static str,
}

const AVAILABLE_PROFILES: &[ProfileInfo] = &[
    ProfileInfo {
        name: "base",
        description: "Core shell environment, no desktop GUI",
    },
    ProfileInfo {
        name: "desktop",
        description: "Full desktop/workstation setup with GUI tools",
    },
];

/// Return the built-in role profiles in display order.
#[must_use]
pub const fn available() -> &'static [ProfileInfo] {
    AVAILABLE_PROFILES
}

pub(super) const KNOWN_CATEGORIES: &[Category] = &[
    Category::Base,
    Category::Desktop,
    Category::Linux,
    Category::Windows,
    Category::Arch,
    Category::Wsl,
];

/// Resolve the profile from CLI arg, `DOTFILES_PROFILE` env var, persisted
/// git config, or interactive prompt.
///
/// When the profile is obtained via interactive prompt it is persisted to the
/// repository's local git config (`dotfiles.profile`) so future runs skip
/// the prompt automatically.
///
/// # Errors
///
/// Returns an error if the profile name is invalid or interactive prompting fails.
pub fn resolve_from_args(
    cli_profile: Option<&str>,
    root: &Path,
    platform: Platform,
    env: &dyn crate::infra::env::Env,
    non_interactive: bool,
) -> Result<Profile> {
    let name = if let Some(name) = selected_profile_name(cli_profile, root, env) {
        name
    } else {
        if non_interactive {
            anyhow::bail!(
                "profile selection is required in non-interactive mode; pass --profile, set DOTFILES_PROFILE, or persist a profile"
            );
        }
        let name = prompt_interactive()?;
        #[allow(clippy::print_stderr, reason = "intentional user-facing output")]
        if let Err(e) = persist(root, &name) {
            eprintln!("warning: could not persist profile to git config: {e}");
        }
        name
    };

    resolve(&name, platform).map_err(Into::into)
}

/// Resolve a profile for a read-only discovery command without prompting or
/// persisting a selection.
///
/// Selection keeps the normal CLI, environment, and repository precedence.
///
/// # Errors
///
/// Returns an error if no profile has already been selected or the selected
/// profile is invalid.
pub fn resolve_read_only(
    cli_profile: Option<&str>,
    root: &Path,
    platform: Platform,
    env: &dyn crate::infra::env::Env,
) -> Result<Profile> {
    let name = selected_profile_name(cli_profile, root, env).ok_or_else(|| {
        anyhow::anyhow!("profile selection is required; pass --profile <base|desktop>")
    })?;
    resolve(&name, platform).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::config::category_matcher::Category;
    use crate::infra::platform::{Os, Platform};

    fn linux_platform() -> Platform {
        Platform::new(Os::Linux, false)
    }

    #[test]
    fn non_interactive_resolution_fails_instead_of_prompting() {
        let (_dir, root) = init_test_repo();

        let error = resolve_from_args(
            None,
            &root,
            linux_platform(),
            &crate::infra::env::MapEnv::new(),
            true,
        )
        .expect_err("non-interactive resolution must not read stdin");

        assert!(
            error.to_string().contains("profile selection is required"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn read_only_resolution_requires_an_existing_selection() {
        let (_dir, root) = init_test_repo();

        let error = resolve_read_only(
            None,
            &root,
            linux_platform(),
            &crate::infra::env::MapEnv::new(),
        )
        .expect_err("discovery should not prompt");
        assert!(error.to_string().contains("pass --profile"));
    }

    #[test]
    fn profiles_include_exactly_the_role_and_platform_categories() {
        use Category::{Arch, Base, Desktop, Linux, Windows, Wsl};

        let platforms: &[(Platform, &[Category])] = &[
            (linux_platform(), &[Linux]),
            (Platform::new(Os::Linux, true), &[Linux, Arch]),
            (Platform::new(Os::Windows, false), &[Windows]),
            (Platform::new_wsl(), &[Linux, Wsl]),
            (
                Platform {
                    os: Os::Linux,
                    is_arch: true,
                    is_wsl: true,
                },
                &[Linux, Arch, Wsl],
            ),
        ];
        for &(platform, categories) in platforms {
            for name in ["base", "desktop"] {
                let profile = resolve(name, platform).unwrap();
                let mut expected = vec![Base];
                if name == "desktop" {
                    expected.push(Desktop);
                }
                expected.extend_from_slice(categories);
                expected.sort();
                assert_eq!(profile.name, name);
                assert_eq!(
                    profile.active_categories, expected,
                    "{name} on {platform:?}"
                );
            }
        }
    }

    #[test]
    fn resolve_unknown_profile_fails() {
        let err = resolve("nonexistent", linux_platform()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("nonexistent"),
            "error should name the bad profile"
        );
        assert!(
            msg.contains("available"),
            "error should list available profiles"
        );
    }

    #[test]
    fn environment_selection_ignores_only_missing_or_empty_values() {
        use crate::infra::env::MapEnv;

        for value in [
            None,
            Some(""),
            Some("desktop"),
            Some("unknown"),
            Some(" base "),
        ] {
            let env = value.map_or_else(MapEnv::new, |value| {
                MapEnv::new().with("DOTFILES_PROFILE", value)
            });
            assert_eq!(
                read_from_env(&env).as_deref(),
                value.filter(|value| !value.is_empty()),
                "{value:?}: nonempty values must be validated, not silently defaulted"
            );
        }
    }

    // ------------------------------------------------------------------
    // persist / read_persisted
    // ------------------------------------------------------------------

    fn init_test_repo() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = git2::Repository::init(dir.path()).expect("git init");
        let root = repo.workdir().unwrap().to_path_buf();
        (dir, root)
    }

    #[test]
    fn read_persisted_returns_none_when_unset() {
        let (dir, root) = init_test_repo();
        let name = read_persisted(&root);
        assert_eq!(name, None);
        drop(dir);
    }

    #[test]
    fn persist_overwrites_previous_value() {
        let (dir, root) = init_test_repo();
        persist(&root, "base").expect("first persist");
        assert_eq!(read_persisted(&root).as_deref(), Some("base"));
        persist(&root, "desktop").expect("second persist");
        let name = read_persisted(&root);
        assert_eq!(name, Some("desktop".to_string()));
        drop(dir);
    }

    #[test]
    fn read_persisted_returns_none_when_repository_is_invalid() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(".git"), "not a git directory").unwrap();
        let name = read_persisted(dir.path());
        assert_eq!(name, None);
    }

    #[test]
    fn read_only_selection_obeys_precedence_without_rewriting_persisted_state() {
        use crate::app::config::error::ConfigError;
        use crate::infra::env::MapEnv;

        let root = tempfile::tempdir_in(".").unwrap();
        let repo = git2::Repository::init(root.path()).unwrap();
        persist(root.path(), "desktop").unwrap();
        let config_path = repo.path().join("config");
        let original = std::fs::read(&config_path).unwrap();

        for (cli, environment, expected) in [
            (Some("base"), Some("invalid"), Ok("base")),
            (None, Some("base"), Ok("base")),
            (None, Some(""), Ok("desktop")),
            (None, None, Ok("desktop")),
            (Some("invalid"), Some("base"), Err("invalid")),
            (Some(""), Some("base"), Err("")),
            (None, Some("invalid"), Err("invalid")),
            (None, Some(" base "), Err(" base ")),
        ] {
            let env = environment.map_or_else(MapEnv::new, |value| {
                MapEnv::new().with("DOTFILES_PROFILE", value)
            });
            let result = resolve_read_only(cli, root.path(), linux_platform(), &env);
            match expected {
                Ok(name) => assert_eq!(result.unwrap().name, name, "{cli:?}/{environment:?}"),
                Err(name) => {
                    let error = result.unwrap_err();
                    let Some(ConfigError::InvalidProfile {
                        name: actual,
                        available,
                    }) = error.downcast_ref::<ConfigError>()
                    else {
                        panic!("expected invalid-profile error, got {error:#}");
                    };
                    assert_eq!(actual, name);
                    assert_eq!(available, "base, desktop");
                }
            }
            assert_eq!(
                std::fs::read(&config_path).unwrap(),
                original,
                "{cli:?}/{environment:?}: discovery must never persist a selection"
            );
        }
    }
}
