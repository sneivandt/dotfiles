use std::collections::HashSet;

use anyhow::Result;

use super::{Resource, ResourceChange, ResourceState};
use crate::exec;

/// Supported package managers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    /// Official Arch Linux packages (pacman).
    Pacman,
    /// AUR packages (paru).
    Paru,
    /// Windows packages (winget).
    Winget,
}

impl std::fmt::Display for PackageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pacman => write!(f, "pacman"),
            Self::Paru => write!(f, "paru"),
            Self::Winget => write!(f, "winget"),
        }
    }
}

/// A system package resource that can be checked and installed.
#[derive(Debug, Clone)]
pub struct PackageResource {
    /// Package name (or winget ID).
    pub name: String,
    /// Package manager to use.
    pub manager: PackageManager,
}

impl PackageResource {
    /// Create a new package resource.
    #[must_use]
    pub const fn new(name: String, manager: PackageManager) -> Self {
        Self { name, manager }
    }

    /// Determine the resource state from a pre-fetched set of installed package names.
    ///
    /// This avoids running a per-package query when used with
    /// [`get_installed_packages`].
    #[must_use]
    pub fn state_from_installed(&self, installed: &HashSet<String>) -> ResourceState {
        if installed.contains(&self.name) {
            ResourceState::Correct
        } else {
            ResourceState::Missing
        }
    }
}

/// Query the full set of installed package names for a given manager.
///
/// Returns a `HashSet` of package names (or winget IDs) that are currently
/// installed. This runs a **single** command regardless of how many packages
/// need to be checked — compared to one command per package when using
/// `PackageResource::current_state()` directly.
///
/// # Errors
///
/// Returns an error if the package manager command fails to execute or if
/// the output cannot be parsed.
pub fn get_installed_packages(manager: PackageManager) -> Result<HashSet<String>> {
    match manager {
        PackageManager::Pacman | PackageManager::Paru => {
            // `pacman -Q` lists all explicitly & dependency-installed packages,
            // one per line: "name version"
            let result = exec::run_unchecked("pacman", &["-Q"])?;
            let mut set = HashSet::new();
            if result.success {
                for line in result.stdout.lines() {
                    if let Some(name) = line.split_whitespace().next() {
                        set.insert(name.to_string());
                    }
                }
            }
            Ok(set)
        }
        PackageManager::Winget => {
            // `winget list` outputs a formatted table — each line may contain
            // the package ID as a whitespace-delimited token.  Winget IDs are
            // reverse-domain names (e.g. `Git.Git`, `Microsoft.PowerShell`) so
            // collisions with version numbers or other tokens are not a concern
            // when doing exact-match lookups via `state_from_installed`.
            let result = exec::run_unchecked(
                "winget",
                &[
                    "list",
                    "--accept-source-agreements",
                    "--disable-interactivity",
                ],
            )?;
            let mut set = HashSet::new();
            if result.success {
                for line in result.stdout.lines() {
                    for token in line.split_whitespace() {
                        set.insert(token.to_string());
                    }
                }
            }
            Ok(set)
        }
    }
}

impl Resource for PackageResource {
    fn description(&self) -> String {
        format!("{} ({})", self.name, self.manager)
    }

    fn current_state(&self) -> Result<ResourceState> {
        match self.manager {
            PackageManager::Pacman | PackageManager::Paru => {
                let result = exec::run_unchecked("pacman", &["-Q", &self.name])?;
                if result.success {
                    Ok(ResourceState::Correct)
                } else {
                    Ok(ResourceState::Missing)
                }
            }
            PackageManager::Winget => {
                let result = exec::run_unchecked(
                    "winget",
                    &[
                        "list",
                        "--id",
                        &self.name,
                        "--exact",
                        "--accept-source-agreements",
                    ],
                )?;
                if result.success && result.stdout.contains(&self.name) {
                    Ok(ResourceState::Correct)
                } else {
                    Ok(ResourceState::Missing)
                }
            }
        }
    }

    fn apply(&self) -> Result<ResourceChange> {
        match self.manager {
            PackageManager::Pacman => {
                exec::run(
                    "sudo",
                    &["pacman", "-S", "--needed", "--noconfirm", &self.name],
                )?;
                Ok(ResourceChange::Applied)
            }
            PackageManager::Paru => {
                exec::run("paru", &["-S", "--needed", "--noconfirm", &self.name])?;
                Ok(ResourceChange::Applied)
            }
            PackageManager::Winget => {
                let result = exec::run_unchecked(
                    "winget",
                    &[
                        "install",
                        "--id",
                        &self.name,
                        "--exact",
                        "--source",
                        "winget",
                        "--accept-source-agreements",
                        "--accept-package-agreements",
                    ],
                )?;
                if result.success {
                    Ok(ResourceChange::Applied)
                } else {
                    // winget writes most diagnostics to stdout, not stderr.
                    // Combine both streams so the user sees useful output.
                    let detail = if result.stderr.trim().is_empty() {
                        result.stdout.trim().to_string()
                    } else {
                        format!("{}\n{}", result.stdout.trim(), result.stderr.trim())
                    };
                    Ok(ResourceChange::Skipped {
                        reason: format!("winget install failed: {detail}"),
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn description_includes_manager() {
        let resource = PackageResource::new("git".to_string(), PackageManager::Pacman);
        assert_eq!(resource.description(), "git (pacman)");

        let resource = PackageResource::new("paru-bin".to_string(), PackageManager::Paru);
        assert_eq!(resource.description(), "paru-bin (paru)");

        let resource = PackageResource::new("Git.Git".to_string(), PackageManager::Winget);
        assert_eq!(resource.description(), "Git.Git (winget)");
    }

    #[test]
    fn state_from_installed_correct() {
        let resource = PackageResource::new("git".to_string(), PackageManager::Pacman);
        let mut installed = HashSet::new();
        installed.insert("git".to_string());
        installed.insert("vim".to_string());
        assert_eq!(
            resource.state_from_installed(&installed),
            ResourceState::Correct
        );
    }

    #[test]
    fn state_from_installed_missing() {
        let resource = PackageResource::new("git".to_string(), PackageManager::Pacman);
        let installed = HashSet::new();
        assert_eq!(
            resource.state_from_installed(&installed),
            ResourceState::Missing
        );
    }
}
