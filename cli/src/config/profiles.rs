//! Profile definition and resolution.
use anyhow::{Context as _, Result, bail};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;

use crate::config::category_matcher::Category;
use crate::error::ConfigError;
use crate::platform::Platform;

/// A resolved profile with its active and excluded categories.
#[derive(Debug, Clone)]
pub struct Profile {
    /// The profile name (e.g., "base", "desktop").
    pub name: String,
    /// Categories that are active for this profile.
    pub active_categories: Vec<Category>,
    /// Categories that are excluded for this profile.
    pub excluded_categories: Vec<Category>,
}

/// Raw profile definition from profiles.toml.
#[derive(Debug, Clone, Deserialize)]
struct ProfileDef {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}

/// Load profile definitions from profiles.toml.
fn load_definitions(path: &Path) -> Result<HashMap<String, ProfileDef>, ConfigError> {
    if !path.exists() {
        return Ok(default_definitions());
    }

    let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
        path: path.display().to_string(),
        source: e,
    })?;

    toml::from_str(&content).map_err(|e| ConfigError::TomlParse {
        path: path.display().to_string(),
        source: e,
    })
}

fn default_definitions() -> HashMap<String, ProfileDef> {
    let mut map = HashMap::new();
    map.insert(
        "base".to_string(),
        ProfileDef {
            description: Some("Core shell environment, no desktop GUI".to_string()),
            include: vec![],
            exclude: vec!["desktop".to_string()],
        },
    );
    map.insert(
        "desktop".to_string(),
        ProfileDef {
            description: Some("Full graphical desktop (Arch + X11)".to_string()),
            include: vec!["desktop".to_string()],
            exclude: vec![],
        },
    );
    map
}

/// Resolve a profile by name: compute the active and excluded categories,
/// applying platform auto-detection overrides.
///
/// # Errors
///
/// Returns an error if the profile is not found or the profiles.toml file cannot be parsed.
pub fn resolve(name: &str, conf_dir: &Path, platform: Platform) -> Result<Profile, ConfigError> {
    let defs = load_definitions(&conf_dir.join("profiles.toml"))?;
    let mut available_names: Vec<&str> = defs.keys().map(String::as_str).collect();
    available_names.sort_unstable();
    let available = available_names.join(", ");
    let def = defs.get(name).ok_or_else(|| ConfigError::InvalidProfile {
        name: name.to_string(),
        available: available.clone(),
    })?;

    // Start with the profile's own include/exclude
    let mut active: Vec<Category> = vec![Category::Base];
    active.extend(def.include.iter().map(|s| Category::from_tag(s)));

    let mut excluded: Vec<Category> = def.exclude.iter().map(|s| Category::from_tag(s)).collect();

    // Auto-add platform-detected categories
    for category in [Category::Linux, Category::Windows, Category::Arch] {
        if !platform.excludes_category(&category) {
            active.push(category);
        } else if !excluded.contains(&category) {
            excluded.push(category);
        }
    }

    // Remove any active categories that are also excluded
    active.retain(|c| !excluded.contains(c));

    // Deduplicate
    active.sort();
    active.dedup();
    excluded.sort();
    excluded.dedup();

    Ok(Profile {
        name: name.to_string(),
        active_categories: active,
        excluded_categories: excluded,
    })
}

/// Try to read the profile from the `DOTFILES_PROFILE` environment variable.
#[must_use]
pub fn read_from_env() -> Option<String> {
    parse_env_profile(std::env::var("DOTFILES_PROFILE").ok())
}

fn parse_env_profile(raw: Option<String>) -> Option<String> {
    raw.filter(|v| !v.is_empty())
}

/// Try to read the persisted profile from the repository's local git config
/// (`dotfiles.profile`).
#[must_use]
pub fn read_persisted(root: &Path) -> Option<String> {
    let repo = git2::Repository::discover(root).ok()?;
    let config = repo.config().ok()?;
    let local = config.open_level(git2::ConfigLevel::Local).ok()?;
    local
        .get_string("dotfiles.profile")
        .ok()
        .filter(|v| !v.is_empty())
}

/// Persist the profile name to the repository's local git config so future
/// runs don't need to prompt interactively.
///
/// # Errors
///
/// Returns an error if the repository cannot be discovered or the config
/// cannot be written.
pub fn persist(root: &Path, name: &str) -> Result<()> {
    let repo = git2::Repository::discover(root).context("finding git repository")?;
    let config = repo.config().context("opening git config")?;
    let mut local = config
        .open_level(git2::ConfigLevel::Local)
        .context("opening local git config")?;
    local
        .set_str("dotfiles.profile", name)
        .context("persisting profile to git config")
}

/// Interactively prompt the user to select a profile.
///
/// Profile names and descriptions are read from `conf/profiles.toml`.
///
/// # Errors
///
/// Returns an error if profiles cannot be loaded or user input cannot be read.
pub fn prompt_interactive(conf_dir: &Path) -> Result<String> {
    let defs = load_definitions(&conf_dir.join("profiles.toml"))?;

    let mut options: Vec<(String, Option<String>)> = defs
        .into_iter()
        .map(|(name, def)| (name, def.description))
        .collect();
    options.sort_by(|(a, _), (b, _)| a.cmp(b));

    if options.is_empty() {
        bail!("no compatible profiles found");
    }

    println!("\nSelect a profile:");
    for (i, (name, desc)) in options.iter().enumerate() {
        if let Some(d) = desc {
            println!("  \x1b[1m{}\x1b[0m) {name} \u{2014} {d}", i + 1);
        } else {
            println!("  \x1b[1m{}\x1b[0m) {name}", i + 1);
        }
    }
    print!("\nProfile [1-{}]: ", options.len());
    io::stdout().flush().context("flushing stdout")?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("reading profile selection")?;

    let choice: usize = input
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid selection"))?;

    if choice == 0 || choice > options.len() {
        bail!("selection out of range");
    }

    options
        .get(choice - 1)
        .map(|(name, _)| name.clone())
        .context("selection out of range")
}

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

    let name = if let Some(name) = cli_profile {
        name.to_string()
    } else if let Some(name) = read_from_env() {
        name
    } else if let Some(name) = read_persisted(root) {
        name
    } else {
        let name = prompt_interactive(&conf_dir)?;
        if let Err(e) = persist(root, &name) {
            eprintln!("warning: could not persist profile to git config: {e}");
        }
        name
    };

    resolve(&name, &conf_dir, platform).map_err(Into::into)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::category_matcher::Category;
    use crate::platform::{Os, Platform};

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
