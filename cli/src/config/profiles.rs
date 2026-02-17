use anyhow::{Result, bail};
use std::io::{self, Write};
use std::path::Path;

use crate::platform::Platform;

/// A resolved profile with its active and excluded categories.
#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    pub active_categories: Vec<String>,
    pub excluded_categories: Vec<String>,
}

/// Raw profile definition from profiles.ini.
#[derive(Debug, Clone)]
struct ProfileDef {
    name: String,
    include: Vec<String>,
    exclude: Vec<String>,
}

/// All known profile names.
pub const PROFILE_NAMES: &[&str] = &["base", "arch", "desktop", "arch-desktop", "windows"];

/// Load profile definitions from profiles.ini.
fn load_definitions(path: &Path) -> Result<Vec<ProfileDef>> {
    let content = if path.exists() {
        std::fs::read_to_string(path)?
    } else {
        return Ok(default_definitions());
    };

    let mut defs = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_include = Vec::new();
    let mut current_exclude = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            // Save previous profile
            if let Some(name) = current_name.take() {
                defs.push(ProfileDef {
                    name,
                    include: current_include.clone(),
                    exclude: current_exclude.clone(),
                });
                current_include.clear();
                current_exclude.clear();
            }
            current_name = Some(trimmed[1..trimmed.len() - 1].to_string());
        } else if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            let vals: Vec<String> = value
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            match key {
                "include" => current_include = vals,
                "exclude" => current_exclude = vals,
                _ => {}
            }
        }
    }

    if let Some(name) = current_name {
        defs.push(ProfileDef {
            name,
            include: current_include,
            exclude: current_exclude,
        });
    }

    Ok(defs)
}

fn default_definitions() -> Vec<ProfileDef> {
    vec![
        ProfileDef {
            name: "base".to_string(),
            include: vec![],
            exclude: vec![
                "windows".to_string(),
                "arch".to_string(),
                "desktop".to_string(),
            ],
        },
        ProfileDef {
            name: "desktop".to_string(),
            include: vec!["desktop".to_string()],
            exclude: vec!["windows".to_string(), "arch".to_string()],
        },
        ProfileDef {
            name: "arch".to_string(),
            include: vec!["arch".to_string()],
            exclude: vec!["windows".to_string(), "desktop".to_string()],
        },
        ProfileDef {
            name: "arch-desktop".to_string(),
            include: vec!["arch".to_string(), "desktop".to_string()],
            exclude: vec!["windows".to_string()],
        },
        ProfileDef {
            name: "windows".to_string(),
            include: vec!["windows".to_string(), "desktop".to_string()],
            exclude: vec!["arch".to_string()],
        },
    ]
}

/// Resolve a profile by name: compute the active and excluded categories,
/// applying platform auto-detection overrides.
pub fn resolve(name: &str, conf_dir: &Path, platform: &Platform) -> Result<Profile> {
    let defs = load_definitions(&conf_dir.join("profiles.ini"))?;
    let def = defs
        .iter()
        .find(|d| d.name == name)
        .ok_or_else(|| anyhow::anyhow!("unknown profile: {name}"))?;

    // Start with the profile's own include/exclude
    let mut active: Vec<String> = vec!["base".to_string()];
    active.extend(def.include.clone());

    let mut excluded = def.exclude.clone();

    // Platform auto-detection overrides
    if platform.excludes_category("windows") && !excluded.contains(&"windows".to_string()) {
        excluded.push("windows".to_string());
    }
    if platform.excludes_category("arch") && !excluded.contains(&"arch".to_string()) {
        excluded.push("arch".to_string());
        active.retain(|c| c != "arch");
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

/// Try to read the persisted profile from git config.
pub fn read_persisted(root: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["config", "--local", "dotfiles.profile"])
        .current_dir(root)
        .output()
        .ok()?;

    if output.status.success() {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !name.is_empty() && PROFILE_NAMES.contains(&name.as_str()) {
            return Some(name);
        }
    }
    None
}

/// Persist the profile selection to git config.
pub fn persist(root: &Path, name: &str) -> Result<()> {
    std::process::Command::new("git")
        .args(["config", "--local", "dotfiles.profile", name])
        .current_dir(root)
        .output()?;
    Ok(())
}

/// Interactively prompt the user to select a profile.
pub fn prompt_interactive(platform: &Platform) -> Result<String> {
    let options: Vec<&str> = PROFILE_NAMES
        .iter()
        .filter(|&&name| {
            // Filter out incompatible profiles
            match name {
                "windows" => platform.is_windows(),
                "arch" | "arch-desktop" => !platform.excludes_category("arch"),
                _ => true,
            }
        })
        .copied()
        .collect();

    if options.is_empty() {
        bail!("no compatible profiles found for this platform");
    }

    println!("\nSelect a profile:");
    for (i, name) in options.iter().enumerate() {
        println!("  \x1b[1m{}\x1b[0m) {name}", i + 1);
    }
    print!("\nProfile [1-{}]: ", options.len());
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let choice: usize = input
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid selection"))?;

    if choice == 0 || choice > options.len() {
        bail!("selection out of range");
    }

    Ok(options[choice - 1].to_string())
}

/// Resolve the profile from CLI arg, git config, or interactive prompt.
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
        prompt_interactive(platform)?
    };

    if !PROFILE_NAMES.contains(&name.as_str()) {
        bail!(
            "unknown profile '{}'. Valid profiles: {}",
            name,
            PROFILE_NAMES.join(", ")
        );
    }

    let profile = resolve(&name, &conf_dir, platform)?;

    // Persist for next time
    persist(root, &name)?;

    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(defs.len(), 5);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"base"));
        assert!(names.contains(&"arch"));
        assert!(names.contains(&"desktop"));
        assert!(names.contains(&"arch-desktop"));
        assert!(names.contains(&"windows"));
    }

    #[test]
    fn resolve_base_on_linux() {
        let dir = std::env::temp_dir();
        let profile = resolve("base", &dir, &linux_platform()).unwrap();
        assert_eq!(profile.name, "base");
        assert!(profile.active_categories.contains(&"base".to_string()));
        assert!(profile.excluded_categories.contains(&"windows".to_string()));
        assert!(profile.excluded_categories.contains(&"arch".to_string()));
        assert!(profile.excluded_categories.contains(&"desktop".to_string()));
    }

    #[test]
    fn resolve_arch_desktop_on_arch() {
        let dir = std::env::temp_dir();
        let profile = resolve("arch-desktop", &dir, &arch_platform()).unwrap();
        assert!(profile.active_categories.contains(&"arch".to_string()));
        assert!(profile.active_categories.contains(&"desktop".to_string()));
        assert!(profile.excluded_categories.contains(&"windows".to_string()));
        assert!(!profile.excluded_categories.contains(&"arch".to_string()));
    }

    #[test]
    fn resolve_arch_on_non_arch_linux() {
        let dir = std::env::temp_dir();
        let profile = resolve("arch", &dir, &linux_platform()).unwrap();
        // arch should be auto-excluded on non-arch linux
        assert!(!profile.active_categories.contains(&"arch".to_string()));
        assert!(profile.excluded_categories.contains(&"arch".to_string()));
    }

    #[test]
    fn resolve_windows_on_windows() {
        let dir = std::env::temp_dir();
        let profile = resolve("windows", &dir, &windows_platform()).unwrap();
        assert!(profile.active_categories.contains(&"windows".to_string()));
        assert!(profile.active_categories.contains(&"desktop".to_string()));
        assert!(!profile.excluded_categories.contains(&"windows".to_string()));
    }

    #[test]
    fn resolve_unknown_profile_fails() {
        let dir = std::env::temp_dir();
        assert!(resolve("nonexistent", &dir, &linux_platform()).is_err());
    }

    #[test]
    fn profile_names_constant() {
        assert_eq!(PROFILE_NAMES.len(), 5);
    }
}
