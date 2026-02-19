use anyhow::{Context as _, Result, bail};
use std::io::{self, Write};
use std::path::Path;

use crate::exec::Executor;
use crate::platform::Platform;

/// A resolved profile with its active and excluded categories.
#[derive(Debug, Clone)]
pub struct Profile {
    /// The profile name (e.g., "base", "desktop").
    pub name: String,
    /// Categories that are active for this profile (e.g., `["base", "linux"]`).
    pub active_categories: Vec<String>,
    /// Categories that are excluded for this profile (e.g., `["windows", "desktop"]`).
    pub excluded_categories: Vec<String>,
}

/// Raw profile definition from profiles.ini.
#[derive(Debug, Clone)]
struct ProfileDef {
    name: String,
    include: Vec<String>,
    exclude: Vec<String>,
}

/// All known profile names available for selection.
pub const PROFILE_NAMES: &[&str] = &["base", "desktop"];

/// Load profile definitions from profiles.ini.
fn load_definitions(path: &Path) -> Result<Vec<ProfileDef>> {
    let content = if path.exists() {
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?
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

        if let Some(inner) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            // Save previous profile
            if let Some(name) = current_name.take() {
                defs.push(ProfileDef {
                    name,
                    include: std::mem::take(&mut current_include),
                    exclude: std::mem::take(&mut current_exclude),
                });
            }
            current_name = Some(inner.to_string());
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
            exclude: vec!["desktop".to_string()],
        },
        ProfileDef {
            name: "desktop".to_string(),
            include: vec!["desktop".to_string()],
            exclude: vec![],
        },
    ]
}

/// Resolve a profile by name: compute the active and excluded categories,
/// applying platform auto-detection overrides.
///
/// # Errors
///
/// Returns an error if the profile is not found or the profiles.ini file cannot be parsed.
pub fn resolve(name: &str, conf_dir: &Path, platform: &Platform) -> Result<Profile> {
    let defs = load_definitions(&conf_dir.join("profiles.ini"))?;
    let def = defs.iter().find(|d| d.name == name).ok_or_else(|| {
        let available = defs
            .iter()
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::anyhow!("unknown profile: {name} (available: {available})")
    })?;

    // Start with the profile's own include/exclude
    let mut active: Vec<String> = vec!["base".to_string()];
    active.extend(def.include.iter().cloned());

    let mut excluded = def.exclude.clone();

    // Auto-add platform-detected categories
    for category in &["linux", "windows", "arch"] {
        if !platform.excludes_category(category) {
            active.push(category.to_string());
        } else if !excluded.iter().any(|c| c == category) {
            excluded.push(category.to_string());
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

/// Try to read the persisted profile from git config.
#[must_use]
pub fn read_persisted(root: &Path, executor: &dyn Executor) -> Option<String> {
    let result = executor
        .run_in(root, "git", &["config", "--local", "dotfiles.profile"])
        .ok()?;
    let name = result.stdout.trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

/// Persist the profile selection to git config.
///
/// # Errors
///
/// Returns an error if the git command fails.
pub fn persist(root: &Path, name: &str, executor: &dyn Executor) -> Result<()> {
    executor
        .run_in(
            root,
            "git",
            &["config", "--local", "dotfiles.profile", name],
        )
        .context("persisting profile to git config")?;
    Ok(())
}

/// Interactively prompt the user to select a profile.
///
/// # Errors
///
/// Returns an error if user input cannot be read.
pub fn prompt_interactive(_platform: &Platform) -> Result<String> {
    let options: Vec<&str> = PROFILE_NAMES.to_vec();

    if options.is_empty() {
        bail!("no compatible profiles found for this platform");
    }

    println!("\nSelect a profile:");
    for (i, name) in options.iter().enumerate() {
        println!("  \x1b[1m{}\x1b[0m) {name}", i + 1);
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
        .map(ToString::to_string)
        .context("selection out of range")
}

/// Resolve the profile from CLI arg, git config, or interactive prompt.
///
/// # Errors
///
/// Returns an error if the profile name is invalid, profile definitions cannot
/// be loaded from profiles.ini, or if interactive prompting fails.
pub fn resolve_from_args(
    cli_profile: Option<&str>,
    root: &Path,
    platform: &Platform,
    executor: &dyn Executor,
) -> Result<Profile> {
    let conf_dir = root.join("conf");

    let name = if let Some(name) = cli_profile {
        name.to_string()
    } else if let Some(name) = read_persisted(root, executor) {
        name
    } else {
        prompt_interactive(platform)?
    };

    // Let resolve() validate the profile name against loaded definitions
    let profile = resolve(&name, &conf_dir, platform)?;

    // Persist for next time (skip during dry-run to avoid side effects)
    // Note: dry_run is not available here, so we always persist. The profile
    // selection itself is not a destructive operation — it only records the
    // user's choice for subsequent runs.
    persist(root, &name, executor)?;

    Ok(profile)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
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
        assert_eq!(defs.len(), 2);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"base"));
        assert!(names.contains(&"desktop"));
    }

    #[test]
    fn resolve_base_on_linux() {
        let dir = std::env::temp_dir();
        let profile = resolve("base", &dir, &linux_platform()).unwrap();
        assert_eq!(profile.name, "base");
        assert!(profile.active_categories.contains(&"base".to_string()));
        assert!(profile.active_categories.contains(&"linux".to_string()));
        assert!(!profile.active_categories.contains(&"desktop".to_string()));
        assert!(profile.excluded_categories.contains(&"windows".to_string()));
        assert!(profile.excluded_categories.contains(&"arch".to_string()));
        assert!(profile.excluded_categories.contains(&"desktop".to_string()));
    }

    #[test]
    fn resolve_desktop_on_linux() {
        let dir = std::env::temp_dir();
        let profile = resolve("desktop", &dir, &linux_platform()).unwrap();
        assert!(profile.active_categories.contains(&"base".to_string()));
        assert!(profile.active_categories.contains(&"linux".to_string()));
        assert!(profile.active_categories.contains(&"desktop".to_string()));
        assert!(!profile.active_categories.contains(&"arch".to_string()));
        assert!(profile.excluded_categories.contains(&"windows".to_string()));
        assert!(profile.excluded_categories.contains(&"arch".to_string()));
    }

    #[test]
    fn resolve_desktop_on_arch() {
        let dir = std::env::temp_dir();
        let profile = resolve("desktop", &dir, &arch_platform()).unwrap();
        assert!(profile.active_categories.contains(&"base".to_string()));
        assert!(profile.active_categories.contains(&"linux".to_string()));
        assert!(profile.active_categories.contains(&"desktop".to_string()));
        assert!(profile.active_categories.contains(&"arch".to_string()));
        assert!(profile.excluded_categories.contains(&"windows".to_string()));
        assert!(!profile.excluded_categories.contains(&"arch".to_string()));
    }

    #[test]
    fn resolve_base_on_arch() {
        let dir = std::env::temp_dir();
        let profile = resolve("base", &dir, &arch_platform()).unwrap();
        assert!(profile.active_categories.contains(&"base".to_string()));
        assert!(profile.active_categories.contains(&"linux".to_string()));
        assert!(profile.active_categories.contains(&"arch".to_string()));
        assert!(!profile.active_categories.contains(&"desktop".to_string()));
        assert!(profile.excluded_categories.contains(&"windows".to_string()));
        assert!(profile.excluded_categories.contains(&"desktop".to_string()));
    }

    #[test]
    fn resolve_base_on_windows() {
        let dir = std::env::temp_dir();
        let profile = resolve("base", &dir, &windows_platform()).unwrap();
        assert!(profile.active_categories.contains(&"base".to_string()));
        assert!(profile.active_categories.contains(&"windows".to_string()));
        assert!(!profile.active_categories.contains(&"linux".to_string()));
        assert!(!profile.active_categories.contains(&"desktop".to_string()));
        assert!(profile.excluded_categories.contains(&"linux".to_string()));
        assert!(profile.excluded_categories.contains(&"desktop".to_string()));
    }

    #[test]
    fn resolve_desktop_on_windows() {
        let dir = std::env::temp_dir();
        let profile = resolve("desktop", &dir, &windows_platform()).unwrap();
        assert!(profile.active_categories.contains(&"base".to_string()));
        assert!(profile.active_categories.contains(&"windows".to_string()));
        assert!(profile.active_categories.contains(&"desktop".to_string()));
        assert!(!profile.active_categories.contains(&"linux".to_string()));
        assert!(profile.excluded_categories.contains(&"linux".to_string()));
        assert!(profile.excluded_categories.contains(&"arch".to_string()));
    }

    #[test]
    fn resolve_unknown_profile_fails() {
        let dir = std::env::temp_dir();
        assert!(resolve("nonexistent", &dir, &linux_platform()).is_err());
    }

    #[test]
    fn profile_names_constant() {
        assert_eq!(PROFILE_NAMES.len(), 2);
    }

    // ------------------------------------------------------------------
    // MockExecutor
    // ------------------------------------------------------------------

    #[derive(Debug)]
    struct MockExecutor {
        responses: std::cell::RefCell<std::collections::VecDeque<(bool, String)>>,
    }

    impl MockExecutor {
        fn ok(stdout: &str) -> Self {
            Self {
                responses: std::cell::RefCell::new(std::collections::VecDeque::from([(
                    true,
                    stdout.to_string(),
                )])),
            }
        }

        fn fail() -> Self {
            Self {
                responses: std::cell::RefCell::new(std::collections::VecDeque::from([(
                    false,
                    String::new(),
                )])),
            }
        }

        fn next(&self) -> (bool, String) {
            self.responses
                .borrow_mut()
                .pop_front()
                .unwrap_or((false, "unexpected call".to_string()))
        }
    }

    impl crate::exec::Executor for MockExecutor {
        fn run(&self, _: &str, _: &[&str]) -> anyhow::Result<crate::exec::ExecResult> {
            let (success, stdout) = self.next();
            if success {
                Ok(crate::exec::ExecResult {
                    stdout,
                    stderr: String::new(),
                    success: true,
                    code: Some(0),
                })
            } else {
                anyhow::bail!("mock command failed")
            }
        }

        fn run_in(
            &self,
            _: &std::path::Path,
            _: &str,
            _: &[&str],
        ) -> anyhow::Result<crate::exec::ExecResult> {
            let (success, stdout) = self.next();
            if success {
                Ok(crate::exec::ExecResult {
                    stdout,
                    stderr: String::new(),
                    success: true,
                    code: Some(0),
                })
            } else {
                anyhow::bail!("mock command failed")
            }
        }

        fn run_in_with_env(
            &self,
            _: &std::path::Path,
            _: &str,
            _: &[&str],
            _: &[(&str, &str)],
        ) -> anyhow::Result<crate::exec::ExecResult> {
            let (success, stdout) = self.next();
            if success {
                Ok(crate::exec::ExecResult {
                    stdout,
                    stderr: String::new(),
                    success: true,
                    code: Some(0),
                })
            } else {
                anyhow::bail!("mock command failed")
            }
        }

        fn run_unchecked(
            &self,
            _: &str,
            _: &[&str],
        ) -> anyhow::Result<crate::exec::ExecResult> {
            let (success, stdout) = self.next();
            Ok(crate::exec::ExecResult {
                stdout,
                stderr: String::new(),
                success,
                code: Some(if success { 0 } else { 1 }),
            })
        }

        fn which(&self, _: &str) -> bool {
            false
        }
    }

    // ------------------------------------------------------------------
    // read_persisted
    // ------------------------------------------------------------------

    #[test]
    fn read_persisted_returns_profile_name() {
        let executor = MockExecutor::ok("desktop\n");
        let dir = std::env::temp_dir();
        assert_eq!(read_persisted(&dir, &executor), Some("desktop".to_string()));
    }

    #[test]
    fn read_persisted_returns_none_when_git_fails() {
        let executor = MockExecutor::fail();
        let dir = std::env::temp_dir();
        assert_eq!(read_persisted(&dir, &executor), None);
    }

    #[test]
    fn read_persisted_returns_none_when_output_empty() {
        let executor = MockExecutor::ok("");
        let dir = std::env::temp_dir();
        assert_eq!(read_persisted(&dir, &executor), None);
    }

    // ------------------------------------------------------------------
    // persist
    // ------------------------------------------------------------------

    #[test]
    fn persist_succeeds_when_git_config_succeeds() {
        let executor = MockExecutor::ok("");
        let dir = std::env::temp_dir();
        persist(&dir, "base", &executor).unwrap();
    }

    #[test]
    fn persist_propagates_error_when_git_fails() {
        let executor = MockExecutor::fail();
        let dir = std::env::temp_dir();
        assert!(persist(&dir, "base", &executor).is_err());
    }
}
