//! Package installation resource.
//!
//! The [`PackageProvider`] trait abstracts over different package managers
//! (pacman, paru, winget). Adding support for a new manager requires a focused
//! provider module under `resources/package/` and a corresponding variant in
//! [`PackageManager`].

#[cfg(test)]
use std::collections::HashMap;
use std::collections::HashSet;
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

    /// Install all missing package resources using this provider's preferred
    /// strategy.
    ///
    /// Providers with native batch support override this to install everything
    /// in one solver invocation. Providers without batch support keep the
    /// default one-at-a-time implementation, which continues after individual
    /// failures and reports them in the returned [`PackageInstallReport`].
    ///
    /// # Errors
    ///
    /// Returns an error if a provider-level batch operation fails.
    fn install_missing(
        &self,
        resources: &[&PackageResource],
        executor: &dyn Executor,
    ) -> Result<PackageInstallReport> {
        let mut report = PackageInstallReport::new();
        for resource in resources {
            match self.install(&resource.name, executor) {
                Ok(ResourceChange::Applied | ResourceChange::AlreadyCorrect) => {
                    report.record_applied(resource.name.clone());
                }
                Ok(ResourceChange::Skipped { reason }) => {
                    report.record_failure(resource.name.clone(), reason);
                }
                Err(err) => {
                    report.record_failure(resource.name.clone(), err.to_string());
                }
            }
        }
        Ok(report)
    }
}

/// Per-package failure captured by [`PackageInstallReport`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInstallFailure {
    /// Package name or ID that failed to install.
    pub package: String,
    /// Human-readable failure reason.
    pub reason: String,
}

/// Outcome of installing a set of missing packages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageInstallReport {
    applied: Vec<String>,
    failures: Vec<PackageInstallFailure>,
}

impl PackageInstallReport {
    /// Create an empty package install report.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            applied: Vec::new(),
            failures: Vec::new(),
        }
    }

    /// Create a report for a successful batch operation.
    #[must_use]
    pub const fn applied(packages: Vec<String>) -> Self {
        Self {
            applied: packages,
            failures: Vec::new(),
        }
    }

    /// Number of packages successfully applied.
    #[must_use]
    pub const fn applied_count(&self) -> usize {
        self.applied.len()
    }

    /// Package names successfully applied.
    #[must_use]
    pub fn applied_packages(&self) -> &[String] {
        &self.applied
    }

    /// Per-package install failures.
    #[must_use]
    pub fn failures(&self) -> &[PackageInstallFailure] {
        &self.failures
    }

    /// Whether any package failed.
    #[must_use]
    pub const fn has_failures(&self) -> bool {
        !self.failures.is_empty()
    }

    fn record_applied(&mut self, package: String) {
        self.applied.push(package);
    }

    fn record_failure(&mut self, package: String, reason: String) {
        self.failures
            .push(PackageInstallFailure { package, reason });
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

/// Install a batch of packages, grouped by package manager.
///
/// Groups the given resources by their [`PackageManager`] and delegates to each
/// provider's preferred missing-package strategy.
///
/// # Errors
///
/// Returns an error if any package manager command fails or if an individual
/// package install is skipped.
#[cfg(test)]
pub fn batch_install_packages(resources: &[&PackageResource]) -> Result<()> {
    let mut groups: HashMap<PackageManager, Vec<&PackageResource>> = HashMap::new();
    for resource in resources {
        groups.entry(resource.manager).or_default().push(resource);
    }

    for (manager, group) in &groups {
        let provider = manager.provider();
        if let Some(first) = group.first() {
            let executor = &*first.executor;

            let report = provider.install_missing(group, executor)?;
            if let Some(failure) = report.failures().first() {
                return Err(crate::error::ResourceError::command_failed(
                    provider.name(),
                    format!(
                        "install failed for '{}': {}",
                        failure.package, failure.reason
                    ),
                )
                .into());
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
            .map_err(|err| {
                crate::error::ResourceError::command_failed(self.provider.name(), err.to_string())
            })
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
