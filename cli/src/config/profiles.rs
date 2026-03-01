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
pub fn resolve(name: &str, conf_dir: &Path, platform: &Platform) -> Result<Profile, ConfigError> {
    let defs = load_definitions(&conf_dir.join("profiles.toml"))?;
    let available = defs
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let def = defs
        .get(name)
        .ok_or_else(|| ConfigError::InvalidProfile(format!("{name} (available: {available})")))?;

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

/// Try to read the persisted profile directly from .git/config.
#[must_use]
pub fn read_persisted(root: &Path) -> Option<String> {
    let content = std::fs::read_to_string(root.join(".git").join("config")).ok()?;
    let mut in_dotfiles = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_dotfiles = trimmed == "[dotfiles]";
            continue;
        }
        if in_dotfiles
            && let Some(rest) = trimmed.strip_prefix("profile")
            && let Some(value) = rest.trim_start().strip_prefix('=')
        {
            let v = value.trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// Persist the profile selection directly to .git/config.
///
/// # Errors
///
/// Returns an error if the config file cannot be read or written.
pub fn persist(root: &Path, name: &str) -> Result<()> {
    let path = root.join(".git").join("config");
    let content = if path.exists() {
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?
    } else {
        String::new()
    };
    let new_content = set_git_config_value(&content, "dotfiles", "profile", name);
    std::fs::write(&path, new_content)
        .with_context(|| format!("writing {}", path.display()))
        .context("persisting profile to git config")?;
    Ok(())
}

/// Update or insert `key = value` within `[section]` in a git config string.
fn set_git_config_value(content: &str, section: &str, key: &str, value: &str) -> String {
    let section_header = format!("[{section}]");
    let new_line = format!("\t{key} = {value}");

    let lines: Vec<&str> = content.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 2);
    let mut in_section = false;
    let mut key_written = false;
    let mut section_seen = false;

    for line in &lines {
        let trimmed = line.trim();
        let entering_new_section = trimmed.starts_with('[');

        if entering_new_section && in_section && !key_written {
            out.push(new_line.clone());
            key_written = true;
        }

        if entering_new_section {
            in_section = trimmed == section_header;
            if in_section {
                section_seen = true;
            }
            out.push(line.to_string());
            continue;
        }

        if in_section
            && !key_written
            && let Some(after_key) = trimmed.strip_prefix(key)
            && after_key.trim_start().starts_with('=')
        {
            out.push(new_line.clone());
            key_written = true;
            continue;
        }

        out.push(line.to_string());
    }

    if in_section && !key_written {
        out.push(new_line.clone());
    }

    if !section_seen {
        if !out.is_empty() {
            out.push(String::new());
        }
        out.push(section_header);
        out.push(new_line);
    }

    let mut result = out.join("\n");
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result
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

/// Resolve the profile from CLI arg, git config, or interactive prompt.
///
/// # Errors
///
/// Returns an error if the profile name is invalid, profile definitions cannot
/// be loaded from profiles.toml, or if interactive prompting fails.
pub fn resolve_from_args(
    cli_profile: Option<&str>,
    root: &Path,
    platform: &Platform,
) -> Result<Profile> {
    let conf_dir = root.join("conf");

    let name = if let Some(name) = cli_profile {
        name.to_string()
    } else if let Some(name) = read_persisted(root) {
        name
    } else {
        prompt_interactive(&conf_dir)?
    };

    // Let resolve() validate the profile name against loaded definitions
    let profile = resolve(&name, &conf_dir, platform)?;

    // Persist for next time (skip during dry-run to avoid side effects)
    // Note: dry_run is not available here, so we always persist. The profile
    // selection itself is not a destructive operation — it only records the
    // user's choice for subsequent runs.
    persist(root, &name)?;

    Ok(profile)
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
        let profile = resolve("base", &dir, &linux_platform()).unwrap();
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
        let profile = resolve("desktop", &dir, &linux_platform()).unwrap();
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
        let profile = resolve("desktop", &dir, &arch_platform()).unwrap();
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
        let profile = resolve("base", &dir, &arch_platform()).unwrap();
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
        let profile = resolve("base", &dir, &windows_platform()).unwrap();
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
        let profile = resolve("desktop", &dir, &windows_platform()).unwrap();
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
        let err = resolve("nonexistent", &dir, &linux_platform()).unwrap_err();
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
    // read_persisted
    // ------------------------------------------------------------------

    #[test]
    fn read_persisted_returns_profile_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let git_dir = dir.path().join(".git");
        std::fs::create_dir_all(&git_dir).expect("create .git");
        std::fs::write(git_dir.join("config"), "[dotfiles]\n\tprofile = desktop\n")
            .expect("write config");
        assert_eq!(read_persisted(dir.path()), Some("desktop".to_string()));
    }

    #[test]
    fn read_persisted_returns_none_when_no_git_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(read_persisted(dir.path()), None);
    }

    #[test]
    fn read_persisted_returns_none_when_profile_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let git_dir = dir.path().join(".git");
        std::fs::create_dir_all(&git_dir).expect("create .git");
        std::fs::write(git_dir.join("config"), "[dotfiles]\n\tprofile = \n").expect("write config");
        assert_eq!(read_persisted(dir.path()), None);
    }

    // ------------------------------------------------------------------
    // persist
    // ------------------------------------------------------------------

    #[test]
    fn persist_creates_section_and_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let git_dir = dir.path().join(".git");
        std::fs::create_dir_all(&git_dir).expect("create .git");
        std::fs::write(git_dir.join("config"), "[core]\n\tbare = false\n").expect("write config");
        persist(dir.path(), "base").expect("persist");
        assert_eq!(read_persisted(dir.path()), Some("base".to_string()));
    }

    #[test]
    fn persist_updates_existing_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let git_dir = dir.path().join(".git");
        std::fs::create_dir_all(&git_dir).expect("create .git");
        std::fs::write(git_dir.join("config"), "[dotfiles]\n\tprofile = base\n")
            .expect("write config");
        persist(dir.path(), "desktop").expect("persist");
        assert_eq!(read_persisted(dir.path()), Some("desktop".to_string()));
    }

    // ------------------------------------------------------------------
    // set_git_config_value
    // ------------------------------------------------------------------

    #[test]
    fn set_git_config_value_creates_section_in_empty_content() {
        let result = set_git_config_value("", "dotfiles", "profile", "base");
        assert!(result.contains("[dotfiles]"), "missing section header");
        assert!(result.contains("profile = base"), "missing key=value");
    }

    #[test]
    fn set_git_config_value_appends_key_to_existing_section() {
        // Section exists but key is absent — the key should be appended inside it.
        let content = "[dotfiles]\n\tother = value\n";
        let result = set_git_config_value(content, "dotfiles", "profile", "desktop");
        assert!(result.contains("profile = desktop"), "key not inserted");
        assert!(
            result.contains("other = value"),
            "existing key must be preserved"
        );
    }

    #[test]
    fn set_git_config_value_updates_existing_key() {
        let content = "[dotfiles]\n\tprofile = base\n";
        let result = set_git_config_value(content, "dotfiles", "profile", "desktop");
        assert!(result.contains("profile = desktop"), "key not updated");
        // The old value must not remain.
        assert!(
            !result.contains("profile = base"),
            "old value should be replaced"
        );
    }

    #[test]
    fn set_git_config_value_preserves_other_sections() {
        let content = "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = git@github.com\n";
        let result = set_git_config_value(content, "dotfiles", "profile", "base");
        assert!(result.contains("[core]"), "core section must be preserved");
        assert!(
            result.contains("bare = false"),
            "core key must be preserved"
        );
        assert!(result.contains("[dotfiles]"), "new section must be added");
    }
}
