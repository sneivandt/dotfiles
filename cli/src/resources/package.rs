use std::collections::HashSet;
use std::sync::OnceLock;

use anyhow::Result;

use super::{Resource, ResourceChange, ResourceState};
use crate::exec::Executor;

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
#[derive(Debug)]
pub struct PackageResource<'a> {
    /// Package name (or winget ID).
    pub name: String,
    /// Package manager to use.
    pub manager: PackageManager,
    /// Executor for running package manager commands.
    executor: &'a dyn Executor,
}

impl<'a> PackageResource<'a> {
    /// Create a new package resource.
    #[must_use]
    pub const fn new(name: String, manager: PackageManager, executor: &'a dyn Executor) -> Self {
        Self {
            name,
            manager,
            executor,
        }
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
pub fn get_installed_packages(
    manager: PackageManager,
    executor: &dyn Executor,
) -> Result<HashSet<String>> {
    match manager {
        PackageManager::Pacman | PackageManager::Paru => {
            // `pacman -Q` lists all explicitly & dependency-installed packages,
            // one per line: "name version"
            let result = executor.run_unchecked("pacman", &["-Q"])?;
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
            let result = executor.run_unchecked(
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

/// Install a batch of packages in a single command, grouped by package manager.
///
/// Groups the given resources by their [`PackageManager`] and runs one
/// installation command per group.  For Pacman packages the command is
/// `sudo pacman -S --needed --noconfirm <names…>`; for Paru packages it is
/// `paru -S --needed --noconfirm <names…>`.  Winget packages are installed
/// individually (winget does not support multi-package installs in one call).
///
/// # Errors
///
/// Returns an error if any package manager command fails.  Already-installed
/// packages are handled gracefully by `--needed` so they do not cause errors.
pub fn batch_install_packages(
    resources: &[&PackageResource<'_>],
    executor: &dyn Executor,
) -> Result<()> {
    let pacman_pkgs: Vec<&str> = resources
        .iter()
        .filter(|r| r.manager == PackageManager::Pacman)
        .map(|r| r.name.as_str())
        .collect();

    let paru_pkgs: Vec<&str> = resources
        .iter()
        .filter(|r| r.manager == PackageManager::Paru)
        .map(|r| r.name.as_str())
        .collect();

    if !pacman_pkgs.is_empty() {
        let mut args = vec!["pacman", "-S", "--needed", "--noconfirm"];
        args.extend(pacman_pkgs);
        executor.run("sudo", &args)?;
    }

    if !paru_pkgs.is_empty() {
        let mut args = vec!["-S", "--needed", "--noconfirm"];
        args.extend(paru_pkgs);
        executor.run("paru", &args)?;
    }

    // Winget does not support batch installs; delegate to individual apply()
    for resource in resources
        .iter()
        .filter(|r| r.manager == PackageManager::Winget)
    {
        resource.apply()?;
    }

    Ok(())
}

/// Lazily-initialized cache of installed packages per package manager.
///
/// Populate with [`PackageCache::get_or_load`] to avoid repeated calls to
/// the package manager when checking many packages.
///
/// # Examples
///
/// ```ignore
/// let cache = PackageCache::new();
/// let installed = cache.get_or_load(PackageManager::Pacman, executor)?;
/// let state = resource.state_from_installed(installed);
/// ```
#[derive(Debug, Default)]
pub struct PackageCache {
    pacman_packages: OnceLock<HashSet<String>>,
    winget_packages: OnceLock<HashSet<String>>,
}

impl PackageCache {
    /// Create an empty cache with no pre-fetched data.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the installed-package set for `manager`, fetching it on first use.
    ///
    /// Subsequent calls for the same manager type return the cached result
    /// without invoking the package manager again.  Pacman and Paru share the
    /// same cache entry because both are queried via `pacman -Q`.
    ///
    /// # Errors
    ///
    /// Returns an error if the package manager command fails on the first call.
    pub fn get_or_load(
        &self,
        manager: PackageManager,
        executor: &dyn Executor,
    ) -> Result<&HashSet<String>> {
        let cell = match manager {
            PackageManager::Pacman | PackageManager::Paru => &self.pacman_packages,
            PackageManager::Winget => &self.winget_packages,
        };
        if cell.get().is_none() {
            let packages = get_installed_packages(manager, executor)?;
            // Ignore a race where another thread beat us to the initialization.
            let _ = cell.set(packages);
        }
        // The cell is guaranteed to be initialized at this point: either we just
        // set it above, or a concurrent caller already did so.
        cell.get().ok_or_else(|| {
            anyhow::anyhow!(
                "BUG: package cache cell for {manager:?} should be initialized after get_or_load"
            )
        })
    }
}

impl Resource for PackageResource<'_> {
    fn description(&self) -> String {
        format!("{} ({})", self.name, self.manager)
    }

    fn current_state(&self) -> Result<ResourceState> {
        match self.manager {
            PackageManager::Pacman | PackageManager::Paru => {
                let result = self.executor.run_unchecked("pacman", &["-Q", &self.name])?;
                if result.success {
                    Ok(ResourceState::Correct)
                } else {
                    Ok(ResourceState::Missing)
                }
            }
            PackageManager::Winget => {
                let result = self.executor.run_unchecked(
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
                self.executor.run(
                    "sudo",
                    &["pacman", "-S", "--needed", "--noconfirm", &self.name],
                )?;
                Ok(ResourceChange::Applied)
            }
            PackageManager::Paru => {
                self.executor
                    .run("paru", &["-S", "--needed", "--noconfirm", &self.name])?;
                Ok(ResourceChange::Applied)
            }
            PackageManager::Winget => {
                let result = self.executor.run_unchecked(
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
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::resources::test_helpers::MockExecutor;

    #[test]
    fn description_includes_manager() {
        let executor = crate::exec::SystemExecutor;
        let resource = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
        assert_eq!(resource.description(), "git (pacman)");

        let resource =
            PackageResource::new("paru-bin".to_string(), PackageManager::Paru, &executor);
        assert_eq!(resource.description(), "paru-bin (paru)");

        let resource =
            PackageResource::new("Git.Git".to_string(), PackageManager::Winget, &executor);
        assert_eq!(resource.description(), "Git.Git (winget)");
    }

    #[test]
    fn state_from_installed_correct() {
        let executor = crate::exec::SystemExecutor;
        let resource = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
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
        let executor = crate::exec::SystemExecutor;
        let resource = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
        let installed = HashSet::new();
        assert_eq!(
            resource.state_from_installed(&installed),
            ResourceState::Missing
        );
    }

    // ------------------------------------------------------------------
    // get_installed_packages
    // ------------------------------------------------------------------

    #[test]
    fn get_installed_pacman_parses_name_version_lines() {
        let executor = MockExecutor::ok("git 2.39.0\nvim 9.0.0\nbase-devel 1.0\n");
        let installed = get_installed_packages(PackageManager::Pacman, &executor).unwrap();
        assert!(installed.contains("git"));
        assert!(installed.contains("vim"));
        assert!(installed.contains("base-devel"));
        assert!(
            !installed.contains("2.39.0"),
            "version number should not be in set"
        );
    }

    #[test]
    fn get_installed_pacman_empty_on_failure() {
        let executor = MockExecutor::fail();
        let installed = get_installed_packages(PackageManager::Pacman, &executor).unwrap();
        assert!(installed.is_empty());
    }

    #[test]
    fn get_installed_winget_parses_id_tokens() {
        let executor = MockExecutor::ok(
            "Name          Id                    Version\nGit           Git.Git               2.39.0\nPowerShell    Microsoft.PowerShell  7.3\n",
        );
        let installed = get_installed_packages(PackageManager::Winget, &executor).unwrap();
        assert!(installed.contains("Git.Git"));
        assert!(installed.contains("Microsoft.PowerShell"));
    }

    // ------------------------------------------------------------------
    // PackageResource::current_state
    // ------------------------------------------------------------------

    #[test]
    fn current_state_pacman_correct_when_query_succeeds() {
        let executor = MockExecutor::ok("git 2.39.0\n");
        let resource = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_pacman_missing_when_query_fails() {
        let executor = MockExecutor::fail();
        let resource = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    #[test]
    fn current_state_winget_correct_when_id_in_output() {
        let executor = MockExecutor::ok("Git.Git  2.39.0\n");
        let resource =
            PackageResource::new("Git.Git".to_string(), PackageManager::Winget, &executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_winget_missing_when_not_in_output() {
        // success=true but ID not present in stdout
        let executor = MockExecutor::ok("No packages found.\n");
        let resource =
            PackageResource::new("Git.Git".to_string(), PackageManager::Winget, &executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    // ------------------------------------------------------------------
    // PackageResource::apply
    // ------------------------------------------------------------------

    #[test]
    fn apply_pacman_returns_applied_on_success() {
        let executor = MockExecutor::ok("");
        let resource = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[test]
    fn apply_paru_returns_applied_on_success() {
        let executor = MockExecutor::ok("");
        let resource =
            PackageResource::new("paru-bin".to_string(), PackageManager::Paru, &executor);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    // ------------------------------------------------------------------
    // batch_install_packages
    // ------------------------------------------------------------------

    #[test]
    fn batch_install_pacman_groups_into_single_command() {
        // One successful response for the batched pacman call
        let executor = MockExecutor::ok("");
        let r1 = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
        let r2 = PackageResource::new("vim".to_string(), PackageManager::Pacman, &executor);
        batch_install_packages(&[&r1, &r2], &executor).unwrap();
    }

    #[test]
    fn batch_install_paru_groups_into_single_command() {
        let executor = MockExecutor::ok("");
        let r1 = PackageResource::new("paru-bin".to_string(), PackageManager::Paru, &executor);
        let r2 = PackageResource::new("yay".to_string(), PackageManager::Paru, &executor);
        batch_install_packages(&[&r1, &r2], &executor).unwrap();
    }

    #[test]
    fn batch_install_mixed_managers_sends_separate_commands() {
        // pacman command + paru command => two successes needed
        let executor = MockExecutor::with_responses(vec![
            (true, String::new()), // pacman batch
            (true, String::new()), // paru batch
        ]);
        let r1 = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
        let r2 = PackageResource::new("paru-bin".to_string(), PackageManager::Paru, &executor);
        batch_install_packages(&[&r1, &r2], &executor).unwrap();
    }

    #[test]
    fn batch_install_empty_list_is_noop() {
        let executor = MockExecutor::fail(); // should never be called
        batch_install_packages(&[], &executor).unwrap();
    }

    #[test]
    fn batch_install_propagates_pacman_error() {
        let executor = MockExecutor::fail();
        let r1 = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
        assert!(batch_install_packages(&[&r1], &executor).is_err());
    }

    // ------------------------------------------------------------------
    // PackageCache
    // ------------------------------------------------------------------

    #[test]
    fn package_cache_fetches_and_caches() {
        // Two OnceLock cells (pacman, winget); only one fetch per manager
        let executor = MockExecutor::ok("git 2.39.0\nvim 9.0.0\n");
        let cache = PackageCache::new();

        let installed = cache
            .get_or_load(PackageManager::Pacman, &executor)
            .unwrap();
        assert!(installed.contains("git"));
        assert!(installed.contains("vim"));

        // Second call should hit the cache — MockExecutor queue is empty,
        // so a fresh command would return "unexpected call" (failure).
        let cached = cache
            .get_or_load(PackageManager::Pacman, &executor)
            .unwrap();
        assert!(cached.contains("git"));
    }

    #[test]
    fn package_cache_paru_shares_pacman_cache() {
        let executor = MockExecutor::ok("git 2.39.0\n");
        let cache = PackageCache::new();

        // Load via Pacman first
        cache
            .get_or_load(PackageManager::Pacman, &executor)
            .unwrap();

        // Querying via Paru must reuse the same cell (no second command)
        let cached = cache.get_or_load(PackageManager::Paru, &executor).unwrap();
        assert!(cached.contains("git"));
    }
}
