//! Package installation resource.
//!
//! The [`PackageProvider`] trait abstracts over different package managers
//! (pacman, paru, winget). Adding support for a new manager requires a focused
//! provider module under `resources/package/` and a corresponding variant in
//! [`PackageManager`].

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;

use super::{Resource, ResourceChange, ResourceResult, ResourceState};
use crate::exec::Executor;

mod pacman;
mod paru;
mod winget;
use pacman::PacmanProvider;
use paru::ParuProvider;
use winget::WingetProvider;
#[cfg(test)]
use winget::parse_winget_ids;

// ---------------------------------------------------------------------------
// PackageProvider trait
// ---------------------------------------------------------------------------

/// Abstraction over package manager operations.
///
/// Each implementation encapsulates the command-line interface of a specific
/// package manager, allowing new managers to be added without modifying the
/// core resource processing logic.
///
/// See [`PacmanProvider`], [`ParuProvider`], and [`WingetProvider`] for
/// concrete implementations.
pub trait PackageProvider: std::fmt::Debug + Send + Sync {
    /// Human-readable name of this provider (e.g., `"pacman"`).
    fn name(&self) -> &'static str;

    /// Query all currently installed package names.
    ///
    /// Returns a set of names/IDs that can be matched against desired
    /// package names to determine what is already installed. Runs a
    /// **single** command regardless of how many packages need checking.
    ///
    /// # Errors
    ///
    /// Returns an error if the package manager command fails or output
    /// cannot be parsed.
    fn query_installed(&self, executor: &dyn Executor) -> Result<HashSet<String>>;

    /// Install a single package.
    ///
    /// # Errors
    ///
    /// Returns an error if the installation command fails.
    fn install(&self, name: &str, executor: &dyn Executor) -> Result<ResourceChange>;

    /// Whether this provider supports installing multiple packages in one command.
    fn supports_batch(&self) -> bool {
        false
    }

    /// Install multiple packages in a single invocation.
    ///
    /// Only called when [`supports_batch`](Self::supports_batch) returns `true`.
    ///
    /// # Errors
    ///
    /// Returns an error if the batch installation command fails.
    fn batch_install(&self, _names: &[&str], _executor: &dyn Executor) -> Result<()> {
        anyhow::bail!("batch install not supported by {}", self.name())
    }
}

// ---------------------------------------------------------------------------
// PackageManager enum
// ---------------------------------------------------------------------------

/// Supported package managers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackageManager {
    /// Official Arch Linux packages (pacman).
    Pacman,
    /// AUR packages (paru).
    Paru,
    /// Windows packages (winget).
    Winget,
}

impl PackageManager {
    /// Return the [`PackageProvider`] implementation for this manager.
    #[must_use]
    pub fn provider(self) -> &'static dyn PackageProvider {
        match self {
            Self::Pacman => &PacmanProvider,
            Self::Paru => &ParuProvider,
            Self::Winget => &WingetProvider,
        }
    }
}

impl std::fmt::Display for PackageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.provider().name())
    }
}

/// A system package resource that can be checked and installed.
#[derive(Debug)]
pub struct PackageResource {
    /// Package name (or winget ID).
    pub name: String,
    /// Package manager to use.
    pub manager: PackageManager,
    /// Provider implementation for this package manager.
    provider: &'static dyn PackageProvider,
    /// Executor for running package manager commands.
    executor: Arc<dyn Executor>,
}

impl PackageResource {
    /// Create a new package resource.
    #[must_use]
    pub fn new(name: String, manager: PackageManager, executor: Arc<dyn Executor>) -> Self {
        Self {
            name,
            manager,
            provider: manager.provider(),
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
/// Delegates to the manager's [`PackageProvider::query_installed`]
/// implementation, running a **single** command regardless of how many
/// packages need to be checked.
///
/// # Errors
///
/// Returns an error if the package manager command fails to execute or if
/// the output cannot be parsed.
pub fn get_installed_packages(
    manager: PackageManager,
    executor: &dyn Executor,
) -> Result<HashSet<String>> {
    manager.provider().query_installed(executor)
}

/// Install a batch of packages in a single command, grouped by package manager.
///
/// Groups the given resources by their [`PackageManager`] and delegates to
/// each provider's batch or individual install method. Providers that support
/// batch installation (pacman, paru) install all missing packages in one
/// command; providers that do not (winget) install individually.
///
/// # Errors
///
/// Returns an error if any package manager command fails, or if a Winget
/// install is skipped (i.e. the installer reported failure).
pub fn batch_install_packages(resources: &[&PackageResource]) -> Result<()> {
    let mut groups: HashMap<PackageManager, Vec<&PackageResource>> = HashMap::new();
    for resource in resources {
        groups.entry(resource.manager).or_default().push(resource);
    }

    for (manager, group) in &groups {
        let provider = manager.provider();
        if let Some(first) = group.first() {
            let executor = &*first.executor;

            if provider.supports_batch() {
                let names: Vec<&str> = group.iter().map(|r| r.name.as_str()).collect();
                provider.batch_install(&names, executor)?;
            } else {
                // Individual install — propagate skipped installations as errors.
                for resource in group {
                    let change = resource.apply()?;
                    if let ResourceChange::Skipped { reason } = change {
                        return Err(crate::error::ResourceError::command_failed(
                            provider.name(),
                            format!("install failed for '{}': {reason}", resource.name),
                        )
                        .into());
                    }
                }
            }
        }
    }

    Ok(())
}

impl Resource for PackageResource {
    fn description(&self) -> String {
        format!("{} ({})", self.name, self.manager)
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        self.provider
            .install(&self.name, &*self.executor)
            .map_err(Into::into)
    }
}

#[cfg(test)]
#[path = "tests/package.rs"]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests;
