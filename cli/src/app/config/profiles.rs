//! Profile selection and resolution.

mod definitions;
mod environment;
mod persistence;
mod prompt;
mod resolution;

use anyhow::Result;
use std::path::Path;

use crate::app::config::error::ConfigError;
use crate::infra::config::category_matcher::Category;
use crate::infra::platform::Platform;

use definitions::load_definitions;
use prompt::prompt_interactive_with_defs;
use resolution::resolve_with_defs;

pub use environment::read_from_env;
pub use persistence::{persist, read_persisted};
pub use prompt::prompt_interactive;
pub use resolution::Profile;
pub use resolution::resolve;

fn definitions(
    conf_dir: &Path,
) -> Result<std::collections::HashMap<String, definitions::ProfileDef>, ConfigError> {
    load_definitions(&conf_dir.join("profiles.toml"))
}

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

/// One configured role profile shown by `dotfiles profiles`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileInfo {
    /// Profile name used with `--profile`.
    pub name: String,
    /// Optional user-facing description from `profiles.toml`.
    pub description: Option<String>,
}

/// Load configured role profiles in stable name order.
///
/// # Errors
///
/// Returns an error when `profiles.toml` cannot be read or parsed.
pub fn available(conf_dir: &Path) -> Result<Vec<ProfileInfo>, ConfigError> {
    let definitions = definitions(conf_dir)?;
    let mut profiles = definitions
        .into_iter()
        .map(|(name, definition)| ProfileInfo {
            name,
            description: definition.description,
        })
        .collect::<Vec<_>>();
    profiles.sort_unstable_by(|left, right| left.name.cmp(&right.name));
    Ok(profiles)
}

pub(super) fn configured_categories(conf_dir: &Path) -> Result<Vec<Category>, ConfigError> {
    let definitions = definitions(conf_dir)?;
    let mut categories = vec![
        Category::Base,
        Category::Desktop,
        Category::Linux,
        Category::Windows,
        Category::Arch,
    ];
    categories.extend(
        definitions
            .values()
            .flat_map(|definition| definition.include.iter().chain(&definition.exclude))
            .map(|tag| Category::from_tag(tag)),
    );
    categories.sort();
    categories.dedup();
    Ok(categories)
}

#[cfg(test)]
use definitions::default_definitions;
#[cfg(test)]
use environment::parse_env_profile;

/// Resolve the profile from CLI arg, `DOTFILES_PROFILE` env var, persisted
/// git config, or interactive prompt.
///
/// When the profile is obtained via interactive prompt it is persisted to the
/// repository's local git config (`dotfiles.profile`) so future runs skip
/// the prompt automatically.
///
/// # Errors
///
/// Returns an error if the profile name is invalid, profile definitions cannot
/// be loaded from profiles.toml, or if interactive prompting fails.
pub fn resolve_from_args(
    cli_profile: Option<&str>,
    root: &Path,
    platform: Platform,
    env: &dyn crate::infra::env::Env,
    non_interactive: bool,
) -> Result<Profile> {
    let conf_dir = root.join("conf");
    let defs = definitions(&conf_dir)?;

    let name = if let Some(name) = selected_profile_name(cli_profile, root, env) {
        name
    } else {
        if non_interactive {
            anyhow::bail!(
                "profile selection is required in non-interactive mode; pass --profile, set DOTFILES_PROFILE, or persist a profile"
            );
        }
        let name = prompt_interactive_with_defs(&defs)?;
        #[allow(clippy::print_stderr, reason = "intentional user-facing output")]
        if let Err(e) = persist(root, &name) {
            eprintln!("warning: could not persist profile to git config: {e}");
        }
        name
    };

    resolve_with_defs(&name, &defs, platform).map_err(Into::into)
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
    let definitions = definitions(&root.join("conf"))?;
    let name = selected_profile_name(cli_profile, root, env).ok_or_else(|| {
        anyhow::anyhow!("profile selection is required; pass --profile or run 'dotfiles profiles'")
    })?;
    resolve_with_defs(&name, &definitions, platform).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::config::category_matcher::Category;
    use crate::infra::platform::{Os, Platform};

    fn linux_platform() -> Platform {
        Platform::new(Os::Linux, false)
    }

    fn arch_platform() -> Platform {
        Platform::new(Os::Linux, true)
    }

    fn windows_platform() -> Platform {
        Platform::new(Os::Windows, false)
    }

    #[test]
    fn non_interactive_resolution_fails_instead_of_prompting() {
        let root = tempfile::tempdir().expect("tempdir");
        let conf = root.path().join("conf");
        std::fs::create_dir(&conf).expect("create conf");
        std::fs::write(
            conf.join("profiles.toml"),
            "[base]\ninclude = []\nexclude = [\"desktop\"]\n",
        )
        .expect("write profiles");

        let error = resolve_from_args(
            None,
            root.path(),
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
    fn available_profiles_are_sorted_with_descriptions() {
        let root = tempfile::tempdir().expect("tempdir");
        let conf = root.path().join("conf");
        std::fs::create_dir(&conf).expect("create conf");
        std::fs::write(
            conf.join("profiles.toml"),
            "[desktop]\ndescription = 'GUI'\n[base]\ndescription = 'CLI'\n",
        )
        .expect("write profiles");

        assert_eq!(
            available(&conf).expect("available profiles"),
            vec![
                ProfileInfo {
                    name: "base".to_string(),
                    description: Some("CLI".to_string()),
                },
                ProfileInfo {
                    name: "desktop".to_string(),
                    description: Some("GUI".to_string()),
                },
            ]
        );
    }

    #[test]
    fn read_only_resolution_requires_an_existing_selection() {
        let root = tempfile::tempdir().expect("tempdir");
        let conf = root.path().join("conf");
        std::fs::create_dir(&conf).expect("create conf");
        std::fs::write(
            conf.join("profiles.toml"),
            "[base]\ninclude = []\nexclude = []\n",
        )
        .expect("write profiles");

        let error = resolve_read_only(
            None,
            root.path(),
            linux_platform(),
            &crate::infra::env::MapEnv::new(),
        )
        .expect_err("discovery should not prompt");
        assert!(error.to_string().contains("pass --profile"));
    }

    fn resolve_default(
        name: &str,
        platform: Platform,
    ) -> std::result::Result<Profile, ConfigError> {
        resolve_with_defs(name, &default_definitions(), platform)
    }

    #[test]
    fn default_definitions_has_all_profiles() {
        let defs = default_definitions();
        assert_eq!(defs.len(), 2);
        let names: Vec<&str> = defs.keys().map(String::as_str).collect();
        assert!(names.contains(&"base"));
        assert!(names.contains(&"desktop"));
    }

    #[test]
    fn resolve_base_on_linux() {
        let profile = resolve_default("base", linux_platform()).unwrap();
        assert_eq!(profile.name, "base");
        assert!(profile.active_categories.contains(&Category::Base));
        assert!(profile.active_categories.contains(&Category::Linux));
        assert!(!profile.active_categories.contains(&Category::Desktop));
        assert!(profile.excluded_categories.contains(&Category::Windows));
        assert!(profile.excluded_categories.contains(&Category::Arch));
        assert!(profile.excluded_categories.contains(&Category::Desktop));
    }

    #[test]
    fn resolve_desktop_on_linux() {
        let profile = resolve_default("desktop", linux_platform()).unwrap();
        assert!(profile.active_categories.contains(&Category::Base));
        assert!(profile.active_categories.contains(&Category::Linux));
        assert!(profile.active_categories.contains(&Category::Desktop));
        assert!(!profile.active_categories.contains(&Category::Arch));
        assert!(profile.excluded_categories.contains(&Category::Windows));
        assert!(profile.excluded_categories.contains(&Category::Arch));
    }

    #[test]
    fn resolve_desktop_on_arch() {
        let profile = resolve_default("desktop", arch_platform()).unwrap();
        assert!(profile.active_categories.contains(&Category::Base));
        assert!(profile.active_categories.contains(&Category::Linux));
        assert!(profile.active_categories.contains(&Category::Desktop));
        assert!(profile.active_categories.contains(&Category::Arch));
        assert!(profile.excluded_categories.contains(&Category::Windows));
        assert!(!profile.excluded_categories.contains(&Category::Arch));
    }

    #[test]
    fn resolve_base_on_arch() {
        let profile = resolve_default("base", arch_platform()).unwrap();
        assert!(profile.active_categories.contains(&Category::Base));
        assert!(profile.active_categories.contains(&Category::Linux));
        assert!(profile.active_categories.contains(&Category::Arch));
        assert!(!profile.active_categories.contains(&Category::Desktop));
        assert!(profile.excluded_categories.contains(&Category::Windows));
        assert!(profile.excluded_categories.contains(&Category::Desktop));
    }

    #[test]
    fn resolve_base_on_windows() {
        let profile = resolve_default("base", windows_platform()).unwrap();
        assert!(profile.active_categories.contains(&Category::Base));
        assert!(profile.active_categories.contains(&Category::Windows));
        assert!(!profile.active_categories.contains(&Category::Linux));
        assert!(!profile.active_categories.contains(&Category::Desktop));
        assert!(profile.excluded_categories.contains(&Category::Linux));
        assert!(profile.excluded_categories.contains(&Category::Desktop));
    }

    #[test]
    fn resolve_desktop_on_windows() {
        let profile = resolve_default("desktop", windows_platform()).unwrap();
        assert!(profile.active_categories.contains(&Category::Base));
        assert!(profile.active_categories.contains(&Category::Windows));
        assert!(profile.active_categories.contains(&Category::Desktop));
        assert!(!profile.active_categories.contains(&Category::Linux));
        assert!(profile.excluded_categories.contains(&Category::Linux));
        assert!(profile.excluded_categories.contains(&Category::Arch));
    }

    #[test]
    fn resolve_unknown_profile_fails() {
        let err = resolve_default("nonexistent", linux_platform()).unwrap_err();
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

    // ------------------------------------------------------------------
    // parse_env_profile (backing read_from_env)
    // ------------------------------------------------------------------

    #[test]
    fn parse_env_profile_returns_some_for_valid_name() {
        assert_eq!(
            parse_env_profile(Some("desktop".to_string())),
            Some("desktop".to_string())
        );
    }

    #[test]
    fn parse_env_profile_returns_none_for_none() {
        assert_eq!(parse_env_profile(None), None);
    }

    #[test]
    fn parse_env_profile_returns_none_for_empty_string() {
        assert_eq!(parse_env_profile(Some(String::new())), None);
    }

    // ------------------------------------------------------------------
    // load_definitions error cases
    // ------------------------------------------------------------------

    #[test]
    fn load_definitions_returns_error_on_malformed_toml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profiles.toml");
        std::fs::write(&path, "[base\ninclude = []\n").expect("write invalid toml");
        let result = load_definitions(&path);
        assert!(result.is_err(), "malformed TOML should return error");
    }

    #[test]
    fn load_definitions_returns_error_on_type_mismatch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profiles.toml");
        std::fs::write(&path, "[base]\ninclude = 42\n").expect("write invalid toml");
        let result = load_definitions(&path);
        assert!(
            result.is_err(),
            "integer instead of array should return error"
        );
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
    fn persist_and_read_persisted_round_trip() {
        let (dir, root) = init_test_repo();
        persist(&root, "desktop").expect("persist should succeed");
        let name = read_persisted(&root);
        assert_eq!(name, Some("desktop".to_string()));
        drop(dir);
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
        persist(&root, "desktop").expect("second persist");
        let name = read_persisted(&root);
        assert_eq!(name, Some("desktop".to_string()));
        drop(dir);
    }

    #[test]
    fn read_persisted_returns_none_outside_git_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let name = read_persisted(dir.path());
        assert_eq!(name, None);
    }
}
