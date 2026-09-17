//! Partial resource-batch accounting retained across error boundaries.

use anyhow::Result;

use super::stats::ItemOutcome;
use super::{TaskResult, TaskStats};
use crate::infra::exec::ExecError;

/// Why resource processing stopped before normal completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchCompletion {
    /// At least one resource failed.
    Failed,
    /// Cancellation prevented the remaining work from completing.
    Interrupted,
}

/// Partial results attached to an unsuccessful resource batch's error.
///
/// Successful batches retain [`TaskResult::Batch`]. On failure, this report is
/// attached as typed `anyhow` context so callers can downcast both the report
/// and the original error without discarding completed work.
#[derive(Debug, thiserror::Error)]
#[error("resource batch did not complete")]
pub struct BatchReport {
    stats: TaskStats,
    completion: BatchCompletion,
    interrupted: u32,
    not_attempted: u32,
}

impl BatchReport {
    /// Outcomes of resources that finished, including failures.
    #[must_use]
    pub const fn stats(&self) -> &TaskStats {
        &self.stats
    }

    /// Whether the batch failed or was interrupted without a failure.
    #[must_use]
    pub const fn completion(&self) -> BatchCompletion {
        self.completion
    }

    /// Resources whose in-flight processing was interrupted.
    #[must_use]
    pub const fn interrupted_count(&self) -> u32 {
        self.interrupted
    }

    /// Resources that were not dispatched after processing stopped.
    #[must_use]
    pub const fn not_attempted_count(&self) -> u32 {
        self.not_attempted
    }

    pub(crate) fn summary(&self, dry_run: bool) -> String {
        let mut parts = vec![self.stats.summary(dry_run)];
        if self.interrupted > 0 {
            parts.push(format!("{} interrupted", self.interrupted));
        }
        if self.not_attempted > 0 {
            parts.push(format!("{} not attempted", self.not_attempted));
        }
        parts.join(", ")
    }
}

/// Detect typed interruption without turning an unrelated failure into one.
pub(crate) fn is_interrupted(error: &anyhow::Error) -> bool {
    if let Some(report) = error.downcast_ref::<BatchReport>() {
        return report.completion == BatchCompletion::Interrupted;
    }
    error.chain().any(|cause| {
        cause
            .downcast_ref::<ExecError>()
            .is_some_and(ExecError::is_cancelled)
            || cause
                .downcast_ref::<super::resource::ResourceError>()
                .is_some_and(super::resource::ResourceError::is_cancelled)
    })
}

/// A fold that retains every completed worker's counts even when another fails.
#[derive(Debug, Default)]
pub(super) struct BatchProgress {
    stats: TaskStats,
    error: Option<anyhow::Error>,
    interrupted: u32,
    not_attempted: u32,
}

impl BatchProgress {
    /// Record a dispatched resource, returning whether dispatch should stop.
    pub(super) fn record(&mut self, result: Result<TaskStats>) -> bool {
        match result {
            Ok(stats) => {
                self.stats.merge(&stats);
                false
            }
            Err(error) => {
                if is_interrupted(&error) {
                    self.interrupted = self.interrupted.saturating_add(1);
                } else {
                    self.stats.record(ItemOutcome::Failed);
                }
                self.retain_error(Some(error));
                true
            }
        }
    }

    pub(super) fn omit(&mut self, count: usize) {
        self.not_attempted = self
            .not_attempted
            .saturating_add(u32::try_from(count).unwrap_or(u32::MAX));
    }

    pub(super) fn merge(mut self, other: Self) -> Self {
        self.stats.merge(&other.stats);
        self.interrupted = self.interrupted.saturating_add(other.interrupted);
        self.not_attempted = self.not_attempted.saturating_add(other.not_attempted);
        self.retain_error(other.error);
        self
    }

    fn retain_error(&mut self, error: Option<anyhow::Error>) {
        if self.error.is_none()
            || (self.error.as_ref().is_some_and(is_interrupted)
                && error.as_ref().is_some_and(|error| !is_interrupted(error)))
        {
            self.error = error;
        }
    }

    pub(super) fn finish(self) -> Result<TaskResult> {
        if self.error.is_none() && self.interrupted == 0 && self.not_attempted == 0 {
            return Ok(self.stats.finish());
        }
        let completion = if self.stats.failed_count() > 0 {
            BatchCompletion::Failed
        } else {
            BatchCompletion::Interrupted
        };
        let report = BatchReport {
            stats: self.stats,
            completion,
            interrupted: self.interrupted,
            not_attempted: self.not_attempted,
        };
        Err(match self.error {
            Some(error) => error.context(report),
            None => anyhow::Error::new(report),
        })
    }
}
