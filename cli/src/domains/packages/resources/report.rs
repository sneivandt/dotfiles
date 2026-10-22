//! Package installation outcome reporting.

use crate::engine::{BatchCompletion, BatchReport, TaskStats};
use crate::infra::exec::ExecError;

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
    already_correct: Vec<String>,
    failures: Vec<PackageInstallFailure>,
}

impl PackageInstallReport {
    /// Create an empty package install report.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            applied: Vec::new(),
            already_correct: Vec::new(),
            failures: Vec::new(),
        }
    }

    /// Create a report for a successful batch operation.
    #[must_use]
    pub const fn applied(packages: Vec<String>) -> Self {
        Self {
            applied: packages,
            already_correct: Vec::new(),
            failures: Vec::new(),
        }
    }

    /// Number of packages successfully applied.
    #[must_use]
    pub const fn applied_count(&self) -> usize {
        self.applied.len()
    }

    /// Number of requested packages that were already installed.
    #[must_use]
    pub const fn already_correct_count(&self) -> usize {
        self.already_correct.len()
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

    pub(crate) fn stats(&self) -> TaskStats {
        TaskStats::from_counts(
            u32::try_from(self.applied_count()).unwrap_or(u32::MAX),
            u32::try_from(self.already_correct_count()).unwrap_or(u32::MAX),
            0,
            u32::try_from(self.failures.len()).unwrap_or(u32::MAX),
        )
    }

    pub(super) fn stopped(
        self,
        mut error: anyhow::Error,
        interrupted: usize,
        not_attempted: usize,
    ) -> anyhow::Error {
        let completion = if !self.has_failures()
            && error
                .downcast_ref::<ExecError>()
                .is_some_and(ExecError::is_cancelled)
        {
            BatchCompletion::Interrupted
        } else {
            BatchCompletion::Failed
        };
        let report = BatchReport::new(
            self.stats(),
            completion,
            u32::try_from(interrupted).unwrap_or(u32::MAX),
            u32::try_from(not_attempted).unwrap_or(u32::MAX),
        );
        for failure in self.failures {
            error = error.context(format!("{}: {}", failure.package, failure.reason));
        }
        error.context(report)
    }

    pub(super) fn record_applied(&mut self, package: String) {
        self.applied.push(package);
    }

    pub(super) fn record_already_correct(&mut self, package: String) {
        self.already_correct.push(package);
    }

    pub(super) fn record_failure(&mut self, package: String, reason: String) {
        self.failures
            .push(PackageInstallFailure { package, reason });
    }
}
