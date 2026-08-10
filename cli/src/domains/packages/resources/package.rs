//! Package installation resource.
//!
//! The [`PackageProvider`] trait abstracts over different package managers
//! (pacman, paru, winget). Adding support for a new manager requires a focused
//! provider module alongside this file and a corresponding variant in
//! [`PackageManager`].

#[cfg(test)]
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;

use crate::engine::{Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::exec::{CommandSpec, Executor};

use super::pacman::PacmanProvider;
use super::paru::ParuProvider;
use super::report::PackageInstallReport;
use super::winget::WingetProvider;

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

    /// Build a single command invocation that installs every name in `names`.
    ///
    /// Providers whose package manager resolves an entire set in one solver
    /// run override this; the default returns `None`, which makes
    /// [`PackageProvider::install_missing`] fall back to installing one
    /// package at a time.
    ///
    /// # Errors
    ///
    /// Returns an error if the invocation cannot be constructed, for example
    /// when a required privilege-escalation helper is unavailable.
    fn batch_invocation<'a>(
        &self,
        names: &[&'a str],
        executor: &dyn Executor,
    ) -> Result<Option<(&'static str, Vec<&'a str>)>> {
        let _ = (names, executor);
        Ok(None)
    }

    /// Install all missing package resources using this provider's preferred
    /// strategy.
    ///
    /// Providers with native batch support (see
    /// [`PackageProvider::batch_invocation`]) install everything in one solver
    /// invocation. Providers without batch support install one at a time,
    /// continuing after individual failures and reporting them in the returned
    /// [`PackageInstallReport`].
    ///
    /// `progress` is called with each package name immediately before its
    /// install starts. One-at-a-time installs can be slow and, on Windows, can
    /// raise a per-installer UAC prompt, so the caller needs to be able to name
    /// the package a stalled run is waiting on rather than reporting only after
    /// every install has finished.
    ///
    /// # Errors
    ///
    /// Returns an error if a provider-level batch operation fails.
    fn install_missing(
        &self,
        resources: &[&PackageResource],
        executor: &dyn Executor,
        progress: &dyn Fn(&str),
    ) -> Result<PackageInstallReport> {
        let names: Vec<&str> = resources
            .iter()
            .map(|resource| resource.name.as_str())
            .collect();

        if let Some((program, args)) = self.batch_invocation(&names, executor)? {
            executor.execute(CommandSpec::new(program).args(&args))?;
            return Ok(PackageInstallReport::applied(
                resources
                    .iter()
                    .map(|resource| resource.name.clone())
                    .collect(),
            ));
        }

        let mut report = PackageInstallReport::new();
        for resource in resources {
            progress(&resource.name);
            match self.install(&resource.name, executor) {
                Ok(ResourceChange::Applied) => {
                    report.record_applied(resource.name.clone());
                }
                Ok(ResourceChange::AlreadyCorrect) => {
                    report.record_already_correct(resource.name.clone());
                }
                Ok(ResourceChange::Skipped {
                    reason,
                    kind: crate::engine::SkipKind::UnmetWork,
                }) => {
                    report.record_failure(resource.name.clone(), reason);
                }
                // A benign skip is neither an install nor a failure, so it
                // contributes to neither bucket in the report.
                Ok(ResourceChange::Skipped {
                    kind: crate::engine::SkipKind::Benign,
                    ..
                }) => {}
                Err(err) => {
                    if err
                        .downcast_ref::<crate::infra::exec::ExecError>()
                        .is_some_and(crate::infra::exec::ExecError::is_cancelled)
                    {
                        return Err(err);
                    }
                    report.record_failure(resource.name.clone(), err.to_string());
                }
            }
        }
        Ok(report)
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
#[derive(Debug, Clone)]
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

/// Install missing package resources through the selected manager's batch API.
///
/// `progress` is called with each package name immediately before its install
/// starts, so callers can show what a long-running install is working on.
///
/// # Errors
///
/// Returns an error when the package manager's batch operation fails before it
/// can produce a per-package report.
pub fn install_missing_packages(
    manager: PackageManager,
    resources: &[&PackageResource],
    executor: &dyn Executor,
    progress: &dyn Fn(&str),
) -> Result<PackageInstallReport> {
    manager
        .provider()
        .install_missing(resources, executor, progress)
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

            let report = provider.install_missing(group, executor, &|_| {})?;
            if let Some(failure) = report.failures().first() {
                return Err(crate::engine::resource::ResourceError::command_failed(
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
                crate::engine::resource::ResourceError::command_failed(
                    self.provider.name(),
                    err.to_string(),
                )
            })
    }
}
