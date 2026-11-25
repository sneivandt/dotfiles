//! Package installation outcome reporting.

use crate::engine::{BatchCompletion, BatchReport, TaskStats};
use crate::infra::exec::ExecError;

/// Per-package installation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageInstallFailure {
    /// Package name or ID that failed to install.
    pub package: String,
    /// Human-readable failure reason.
    pub reason: String,
}

/// Outcome of installing a set of missing packages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PackageInstallReport {
    applied: Vec<String>,
    pending_reboot: Vec<String>,
    already_correct: Vec<String>,
    failures: Vec<PackageInstallFailure>,
}

impl PackageInstallReport {
    /// Create an empty package install report.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            applied: Vec::new(),
            pending_reboot: Vec::new(),
            already_correct: Vec::new(),
            failures: Vec::new(),
        }
    }

    /// Number of packages successfully applied.
    #[must_use]
    pub(crate) const fn applied_count(&self) -> usize {
        self.applied.len() + self.pending_reboot.len()
    }

    /// Number of requested packages that were already installed.
    #[must_use]
    pub(crate) const fn already_correct_count(&self) -> usize {
        self.already_correct.len()
    }

    /// Per-package install failures.
    #[must_use]
    pub(crate) fn failures(&self) -> &[PackageInstallFailure] {
        &self.failures
    }

    pub(crate) fn pending_reboot(&self) -> &[String] {
        &self.pending_reboot
    }

    /// Whether any package failed.
    #[must_use]
    pub(crate) const fn has_failures(&self) -> bool {
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
        for package in self.pending_reboot {
            error = error.context(format!("{package} requires a reboot to finish installing"));
        }
        error.context(report)
    }

    pub(super) fn record_applied(&mut self, package: String) {
        self.applied.push(package);
    }

    pub(super) fn record_pending_reboot(&mut self, package: String) {
        self.pending_reboot.push(package);
    }

    pub(super) fn record_already_correct(&mut self, package: String) {
        self.already_correct.push(package);
    }

    pub(super) fn record_failure(&mut self, package: String, reason: String) {
        self.failures
            .push(PackageInstallFailure { package, reason });
    }
}
