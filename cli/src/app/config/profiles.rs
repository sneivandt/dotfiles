//! Profile selection and resolution.

mod definitions;
mod environment;
mod persistence;
mod prompt;
mod resolution;

use anyhow::Result;
use std::path::Path;

use crate::infra::platform::Platform;

use definitions::load_definitions;
use prompt::prompt_interactive_with_defs;
use resolution::resolve_with_defs;

pub use environment::read_from_env;
pub use persistence::{persist, read_persisted};
#[cfg(any(test, feature = "internal-api", doctest))]
pub use prompt::prompt_interactive;
pub use resolution::Profile;
#[cfg(any(test, feature = "internal-api", doctest))]
pub use resolution::resolve;

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
) -> Result<Profile> {
    let conf_dir = root.join("conf");
    let defs = load_definitions(&conf_dir.join("profiles.toml"))?;

    let name = if let Some(name) = cli_profile
        .map(str::to_owned)
        .or_else(read_from_env)
        .or_else(|| read_persisted(root))
    {
        name
    } else {
        let name = prompt_interactive_with_defs(&defs)?;
        #[allow(clippy::print_stderr, reason = "intentional user-facing output")]
        if let Err(e) = persist(root, &name) {
            eprintln!("warning: could not persist profile to git config: {e}");
        }
        name
    };

    resolve_with_defs(&name, &defs, platform).map_err(Into::into)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
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
    fn default_definitions_has_all_profiles() {
        let defs = default_definitions();
        assert_eq!(defs.len(), 2);
        let names: Vec<&str> = defs.keys().map(String::as_str).collect();
        assert!(names.contains(&"base"));
        assert!(names.contains(&"desktop"));
    }

    #[test]
    fn resolve_base_on_linux() {
        let dir = std::env::temp_dir();
        let profile = resolve("base", &dir, linux_platform()).unwrap();
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
        let dir = std::env::temp_dir();
        let profile = resolve("desktop", &dir, linux_platform()).unwrap();
        assert!(profile.active_categories.contains(&Category::Base));
        assert!(profile.active_categories.contains(&Category::Linux));
        assert!(profile.active_categories.contains(&Category::Desktop));
        assert!(!profile.active_categories.contains(&Category::Arch));
        assert!(profile.excluded_categories.contains(&Category::Windows));
        assert!(profile.excluded_categories.contains(&Category::Arch));
    }

    #[test]
    fn resolve_desktop_on_arch() {
        let dir = std::env::temp_dir();
        let profile = resolve("desktop", &dir, arch_platform()).unwrap();
        assert!(profile.active_categories.contains(&Category::Base));
        assert!(profile.active_categories.contains(&Category::Linux));
        assert!(profile.active_categories.contains(&Category::Desktop));
        assert!(profile.active_categories.contains(&Category::Arch));
        assert!(profile.excluded_categories.contains(&Category::Windows));
        assert!(!profile.excluded_categories.contains(&Category::Arch));
    }

    #[test]
    fn resolve_base_on_arch() {
        let dir = std::env::temp_dir();
        let profile = resolve("base", &dir, arch_platform()).unwrap();
        assert!(profile.active_categories.contains(&Category::Base));
        assert!(profile.active_categories.contains(&Category::Linux));
        assert!(profile.active_categories.contains(&Category::Arch));
        assert!(!profile.active_categories.contains(&Category::Desktop));
        assert!(profile.excluded_categories.contains(&Category::Windows));
        assert!(profile.excluded_categories.contains(&Category::Desktop));
    }

    #[test]
    fn resolve_base_on_windows() {
        let dir = std::env::temp_dir();
        let profile = resolve("base", &dir, windows_platform()).unwrap();
        assert!(profile.active_categories.contains(&Category::Base));
        assert!(profile.active_categories.contains(&Category::Windows));
        assert!(!profile.active_categories.contains(&Category::Linux));
        assert!(!profile.active_categories.contains(&Category::Desktop));
        assert!(profile.excluded_categories.contains(&Category::Linux));
        assert!(profile.excluded_categories.contains(&Category::Desktop));
    }

    #[test]
    fn resolve_desktop_on_windows() {
        let dir = std::env::temp_dir();
        let profile = resolve("desktop", &dir, windows_platform()).unwrap();
        assert!(profile.active_categories.contains(&Category::Base));
        assert!(profile.active_categories.contains(&Category::Windows));
        assert!(profile.active_categories.contains(&Category::Desktop));
        assert!(!profile.active_categories.contains(&Category::Linux));
        assert!(profile.excluded_categories.contains(&Category::Linux));
        assert!(profile.excluded_categories.contains(&Category::Arch));
    }

    #[test]
    fn resolve_unknown_profile_fails() {
        let dir = std::env::temp_dir();
        let err = resolve("nonexistent", &dir, linux_platform()).unwrap_err();
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
