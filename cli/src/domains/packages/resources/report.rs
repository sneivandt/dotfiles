//! Package installation outcome reporting.

/// Per-package installation failure.
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

    pub(super) fn record_applied(&mut self, package: String) {
        self.applied.push(package);
    }

    pub(super) fn record_failure(&mut self, package: String, reason: String) {
        self.failures
            .push(PackageInstallFailure { package, reason });
    }
}
