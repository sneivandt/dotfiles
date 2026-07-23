//! Result and statistics types for task execution.

/// Result of a single task execution.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::testing::tasks::{TaskResult, TaskStats};
///
/// let ok = TaskResult::Ok;
/// let changed = TaskResult::Batch(TaskStats::changed_with_message("installed 1 package"));
/// let na = TaskResult::NotApplicable("nothing configured".into());
/// let skipped = TaskResult::Skipped("not on arch".into());
/// let failed = TaskResult::Failed("git pull failed".into());
///
/// assert!(matches!(ok, TaskResult::Ok));
/// assert!(matches!(changed, TaskResult::Batch(_)));
/// assert!(matches!(na, TaskResult::NotApplicable(_)));
/// assert!(matches!(skipped, TaskResult::Skipped(_)));
/// assert!(matches!(failed, TaskResult::Failed(_)));
/// ```
#[derive(Debug, Clone)]
#[must_use]
pub enum TaskResult {
    /// Task completed successfully.
    Ok,
    /// Validation check completed successfully.
    CheckPassed,
    /// Task is not applicable (e.g., no config matched the active profile).
    NotApplicable(String),
    /// Task was explicitly skipped (e.g., running on a different platform, detached HEAD).
    ///
    /// Skipped indicates a deliberate decision not to act.  Use [`Failed`] when
    /// the task attempted work but did not succeed.
    ///
    /// [`Failed`]: Self::Failed
    Skipped(String),
    /// Task attempted work but encountered a non-fatal failure.
    ///
    /// Unlike [`Skipped`], this variant means the task tried to do something
    /// and did not succeed.  The run continues, but the outcome is recorded
    /// as a failure for visibility.
    ///
    /// [`Skipped`]: Self::Skipped
    Failed(String),
    /// Task processed a batch of actions with structured counters.
    Batch(TaskStats),
}

/// Counters for batch tasks that process many items.
///
/// Provides consistent summary logging across all tasks.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::testing::tasks::TaskStats;
///
/// let mut stats = TaskStats::new();
/// stats.changed = 3;
/// stats.already_ok = 10;
///
/// assert_eq!(stats.summary(false), "3 changed, 10 already ok");
/// assert_eq!(stats.summary(true), "3 would change, 10 already ok");
/// ```
///
/// When items are skipped or fail non-fatally, the summary includes the counts:
///
/// ```
/// use dotfiles_cli::testing::tasks::TaskStats;
///
/// let mut stats = TaskStats::new();
/// stats.changed = 1;
/// stats.already_ok = 2;
/// stats.skipped = 3;
/// stats.failed = 1;
/// assert_eq!(stats.summary(false), "1 changed, 2 already ok, 3 skipped, 1 failed");
/// ```
#[derive(Debug, Clone, Default)]
pub struct TaskStats {
    /// Number of items changed or applied.
    pub changed: u32,
    /// Number of items already in the correct state.
    pub already_ok: u32,
    /// Number of items deliberately skipped due to inapplicability.
    pub skipped: u32,
    /// Number of items that failed without aborting the enclosing task.
    pub failed: u32,
    /// Optional domain-specific summary for this batch.
    pub message: Option<String>,
}

impl TaskStats {
    /// Create a new empty stats counter.
    ///
    /// # Examples
    ///
    /// ```
    /// use dotfiles_cli::testing::tasks::TaskStats;
    ///
    /// let stats = TaskStats::new();
    /// assert_eq!(stats.changed, 0);
    /// assert_eq!(stats.already_ok, 0);
    /// assert_eq!(stats.skipped, 0);
    /// assert_eq!(stats.failed, 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create stats representing one changed item.
    #[must_use]
    pub const fn changed() -> Self {
        Self {
            changed: 1,
            already_ok: 0,
            skipped: 0,
            failed: 0,
            message: None,
        }
    }

    /// Create stats representing one changed item with a descriptive summary.
    pub fn changed_with_message(message: impl Into<String>) -> Self {
        Self {
            message: Some(message.into()),
            ..Self::changed()
        }
    }

    /// Format the summary string (e.g. "3 changed, 10 already ok, 1 skipped").
    ///
    /// # Examples
    ///
    /// ```
    /// use dotfiles_cli::testing::tasks::TaskStats;
    ///
    /// let stats = TaskStats {
    ///     changed: 5,
    ///     already_ok: 12,
    ///     skipped: 0,
    ///     failed: 0,
    ///     message: None,
    /// };
    /// assert_eq!(stats.summary(false), "5 changed, 12 already ok");
    /// assert_eq!(stats.summary(true), "5 would change, 12 already ok");
    /// ```
    #[must_use]
    pub fn summary(&self, dry_run: bool) -> String {
        let verb = if dry_run { "would change" } else { "changed" };
        let mut parts = vec![
            format!("{} {verb}", self.changed),
            format!("{} already ok", self.already_ok),
        ];
        if self.skipped > 0 {
            parts.push(format!("{} skipped", self.skipped));
        }
        if self.failed > 0 {
            parts.push(format!("{} failed", self.failed));
        }
        parts.join(", ")
    }

    /// Merge another stats delta into this one, saturating each counter.
    ///
    /// Prefer this over `+=` at call sites: it performs the same saturating
    /// addition as [`AddAssign`](std::ops::AddAssign) but as a plain method
    /// call, so it does not trip the `arithmetic_side_effects` lint.
    pub const fn merge(&mut self, other: &Self) {
        self.changed = self.changed.saturating_add(other.changed);
        self.already_ok = self.already_ok.saturating_add(other.already_ok);
        self.skipped = self.skipped.saturating_add(other.skipped);
        self.failed = self.failed.saturating_add(other.failed);
    }

    /// Return these counters as a structured task result.
    pub const fn finish(self) -> TaskResult {
        TaskResult::Batch(self)
    }
}

impl std::ops::AddAssign for TaskStats {
    fn add_assign(&mut self, other: Self) {
        self.merge(&other);
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------
    // TaskStats construction
    // -------------------------------------------------------------------

    #[test]
    fn new_stats_are_all_zero() {
        let stats = TaskStats::new();
        assert_eq!(stats.changed, 0);
        assert_eq!(stats.already_ok, 0);
        assert_eq!(stats.skipped, 0);
        assert_eq!(stats.failed, 0);
    }

    #[test]
    fn default_stats_are_all_zero() {
        let stats = TaskStats::default();
        assert_eq!(stats.changed, 0);
        assert_eq!(stats.already_ok, 0);
        assert_eq!(stats.skipped, 0);
        assert_eq!(stats.failed, 0);
    }

    // -------------------------------------------------------------------
    // TaskStats::summary
    // -------------------------------------------------------------------

    #[test]
    fn summary_all_zeros() {
        let stats = TaskStats::new();
        assert_eq!(stats.summary(false), "0 changed, 0 already ok");
    }

    #[test]
    fn summary_all_zeros_dry_run() {
        let stats = TaskStats::new();
        assert_eq!(stats.summary(true), "0 would change, 0 already ok");
    }

    #[test]
    fn summary_changed_only() {
        let stats = TaskStats {
            changed: 5,
            already_ok: 0,
            skipped: 0,
            failed: 0,
            message: None,
        };
        assert_eq!(stats.summary(false), "5 changed, 0 already ok");
    }

    #[test]
    fn summary_already_ok_only() {
        let stats = TaskStats {
            changed: 0,
            already_ok: 10,
            skipped: 0,
            failed: 0,
            message: None,
        };
        assert_eq!(stats.summary(false), "0 changed, 10 already ok");
    }

    #[test]
    fn summary_with_skipped() {
        let stats = TaskStats {
            changed: 1,
            already_ok: 2,
            skipped: 3,
            failed: 0,
            message: None,
        };
        assert_eq!(stats.summary(false), "1 changed, 2 already ok, 3 skipped");
    }

    #[test]
    fn summary_with_skipped_dry_run() {
        let stats = TaskStats {
            changed: 1,
            already_ok: 2,
            skipped: 3,
            failed: 1,
            message: None,
        };
        assert_eq!(
            stats.summary(true),
            "1 would change, 2 already ok, 3 skipped, 1 failed"
        );
    }

    #[test]
    fn summary_hides_skipped_when_zero() {
        let stats = TaskStats {
            changed: 3,
            already_ok: 7,
            skipped: 0,
            failed: 0,
            message: None,
        };
        let s = stats.summary(false);
        assert!(!s.contains("skipped"), "should not mention skipped: {s}");
    }

    // -------------------------------------------------------------------
    // TaskStats::finish
    // -------------------------------------------------------------------

    #[test]
    fn finish_returns_batch_when_changes_were_recorded() {
        let stats = TaskStats {
            changed: 1,
            already_ok: 0,
            skipped: 0,
            failed: 0,
            message: None,
        };
        assert!(matches!(
            stats.finish(),
            TaskResult::Batch(stats) if stats.changed == 1
        ));
    }

    #[test]
    fn finish_returns_batch_when_no_changes_were_recorded() {
        let stats = TaskStats {
            changed: 0,
            already_ok: 1,
            skipped: 0,
            failed: 0,
            message: None,
        };
        assert!(matches!(
            stats.finish(),
            TaskResult::Batch(stats) if stats.already_ok == 1
        ));
    }

    #[test]
    fn finish_returns_batch_when_only_resource_skips_were_recorded() {
        let stats = TaskStats {
            changed: 0,
            already_ok: 0,
            skipped: 1,
            failed: 0,
            message: None,
        };
        assert!(matches!(
            stats.finish(),
            TaskResult::Batch(stats) if stats.skipped == 1
        ));
    }

    #[test]
    fn finish_returns_batch_when_dry_run() {
        let stats = TaskStats {
            changed: 1,
            already_ok: 0,
            skipped: 0,
            failed: 0,
            message: None,
        };
        assert!(matches!(
            stats.finish(),
            TaskResult::Batch(stats) if stats.changed == 1
        ));
    }

    #[test]
    fn finish_returns_batch_when_dry_run_has_no_changes() {
        let stats = TaskStats {
            changed: 0,
            already_ok: 1,
            skipped: 0,
            failed: 0,
            message: None,
        };
        assert!(matches!(
            stats.finish(),
            TaskResult::Batch(stats) if stats.already_ok == 1
        ));
    }

    #[test]
    fn finish_returns_batch_when_non_fatal_failures_were_recorded() {
        let stats = TaskStats {
            changed: 0,
            already_ok: 1,
            skipped: 0,
            failed: 1,
            message: None,
        };

        assert!(matches!(
            stats.finish(),
            TaskResult::Batch(stats) if stats.failed == 1
        ));
    }

    // -------------------------------------------------------------------
    // AddAssign
    // -------------------------------------------------------------------

    #[test]
    fn add_assign_accumulates_all_fields() {
        let mut a = TaskStats {
            changed: 1,
            already_ok: 2,
            skipped: 3,
            failed: 4,
            message: None,
        };
        let b = TaskStats {
            changed: 10,
            already_ok: 20,
            skipped: 30,
            failed: 40,
            message: None,
        };
        a += b;
        assert_eq!(a.changed, 11);
        assert_eq!(a.already_ok, 22);
        assert_eq!(a.skipped, 33);
        assert_eq!(a.failed, 44);
    }

    #[test]
    fn add_assign_with_zero_is_identity() {
        let mut a = TaskStats {
            changed: 5,
            already_ok: 3,
            skipped: 1,
            failed: 2,
            message: None,
        };
        a += TaskStats::new();
        assert_eq!(a.changed, 5);
        assert_eq!(a.already_ok, 3);
        assert_eq!(a.skipped, 1);
        assert_eq!(a.failed, 2);
    }

    // -------------------------------------------------------------------
    // TaskResult variants
    // -------------------------------------------------------------------

    #[test]
    fn task_result_ok_matches() {
        assert!(matches!(TaskResult::Ok, TaskResult::Ok));
    }

    #[test]
    fn task_result_not_applicable_carries_reason() {
        let r = TaskResult::NotApplicable("no config".into());
        match r {
            TaskResult::NotApplicable(reason) => assert_eq!(reason, "no config"),
            other @ (TaskResult::Ok
            | TaskResult::CheckPassed
            | TaskResult::Skipped(_)
            | TaskResult::Failed(_)
            | TaskResult::Batch(_)) => panic!("expected NotApplicable, got {other:?}"),
        }
    }

    #[test]
    fn task_result_skipped_carries_reason() {
        let r = TaskResult::Skipped("wrong platform".into());
        match r {
            TaskResult::Skipped(reason) => assert_eq!(reason, "wrong platform"),
            other @ (TaskResult::Ok
            | TaskResult::CheckPassed
            | TaskResult::NotApplicable(_)
            | TaskResult::Failed(_)
            | TaskResult::Batch(_)) => panic!("expected Skipped, got {other:?}"),
        }
    }

    #[test]
    fn task_result_failed_carries_reason() {
        let r = TaskResult::Failed("git pull failed".into());
        match r {
            TaskResult::Failed(reason) => assert_eq!(reason, "git pull failed"),
            other @ (TaskResult::Ok
            | TaskResult::CheckPassed
            | TaskResult::NotApplicable(_)
            | TaskResult::Skipped(_)
            | TaskResult::Batch(_)) => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn task_result_debug_format() {
        let r = TaskResult::Ok;
        assert_eq!(format!("{r:?}"), "Ok");
    }

    #[test]
    fn task_result_clone() {
        let r = TaskResult::Skipped("reason".into());
        #[allow(clippy::redundant_clone, reason = "clone keeps test intent explicit")]
        let r2 = r.clone();
        assert!(matches!(r2, TaskResult::Skipped(_)));
    }
}
