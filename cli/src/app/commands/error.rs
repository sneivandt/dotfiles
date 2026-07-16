//! Errors raised by command orchestration.

use thiserror::Error;

/// Aggregate failure reported after one or more tasks already logged their
/// individual errors.
#[derive(Error, Debug)]
#[error("{count} task(s) failed")]
pub(crate) struct TaskFailures {
    count: usize,
}

impl TaskFailures {
    /// Create an aggregate task failure for the completed run.
    pub(crate) const fn new(count: usize) -> Self {
        Self { count }
    }
}
