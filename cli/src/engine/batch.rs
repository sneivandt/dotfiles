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
    pub(crate) const fn new(
        stats: TaskStats,
        completion: BatchCompletion,
        interrupted: u32,
        not_attempted: u32,
    ) -> Self {
        Self {
            stats,
            completion,
            interrupted,
            not_attempted,
        }
    }

    pub(crate) const fn merge_stats(&mut self, stats: &TaskStats) {
        self.stats.merge(stats);
    }

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
    if error
        .downcast_ref::<super::access::AccessCancelled>()
        .is_some()
    {
        return true;
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
    pub(super) const fn stopped(&self) -> bool {
        self.error.is_some()
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_summary_only_includes_nonzero_interrupted_and_omitted_counts() {
        let stats = TaskStats::from_counts(1, 2, 0, 0);
        for (interrupted, not_attempted, expected) in [
            (0, 0, "1 changed, 2 already ok"),
            (1, 0, "1 changed, 2 already ok, 1 interrupted"),
            (0, 2, "1 changed, 2 already ok, 2 not attempted"),
            (
                1,
                2,
                "1 changed, 2 already ok, 1 interrupted, 2 not attempted",
            ),
        ] {
            assert_eq!(
                BatchReport::new(
                    stats.clone(),
                    BatchCompletion::Interrupted,
                    interrupted,
                    not_attempted
                )
                .summary(false),
                expected
            );
        }
    }

    #[test]
    fn batch_completion_controls_interruption_classification() {
        for (completion, interrupted) in [
            (BatchCompletion::Failed, false),
            (BatchCompletion::Interrupted, true),
        ] {
            let error = anyhow::Error::new(super::super::access::AccessCancelled)
                .context(BatchReport::new(TaskStats::default(), completion, 1, 0));
            assert_eq!(is_interrupted(&error), interrupted);
        }
    }

    #[test]
    fn merging_later_waves_keeps_the_first_failure_available_to_the_caller() {
        let mut first = BatchProgress::default();
        assert!(first.record(Err(anyhow::anyhow!("original resource failed"))));
        let mut later = BatchProgress::default();
        assert!(later.record(Err(anyhow::anyhow!("later resource failed"))));
        let error = first.merge(later).finish().unwrap_err();
        assert_eq!(error.root_cause().to_string(), "original resource failed");
        assert_eq!(
            error
                .downcast_ref::<BatchReport>()
                .unwrap()
                .stats()
                .failed_count(),
            2
        );
    }
}
