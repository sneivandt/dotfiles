//! Result and statistics types for task execution.

use super::context::Context;

/// Result of a single task execution.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::testing::tasks::TaskResult;
///
/// let ok = TaskResult::Ok;
/// let ok_with_message = TaskResult::OkWithMessage("installed 1 package".into());
/// let na = TaskResult::NotApplicable("nothing configured".into());
/// let skipped = TaskResult::Skipped("not on arch".into());
/// let failed = TaskResult::Failed("git pull failed".into());
/// let dry = TaskResult::DryRun;
///
/// assert!(matches!(ok, TaskResult::Ok));
/// assert!(matches!(ok_with_message, TaskResult::OkWithMessage(_)));
/// assert!(matches!(na, TaskResult::NotApplicable(_)));
/// assert!(matches!(skipped, TaskResult::Skipped(_)));
/// assert!(matches!(failed, TaskResult::Failed(_)));
/// assert!(matches!(dry, TaskResult::DryRun));
/// ```
#[derive(Debug, Clone)]
#[must_use]
pub enum TaskResult {
    /// Task completed successfully.
    Ok,
    /// Task completed successfully with a user-facing detail message.
    OkWithMessage(String),
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
    /// Task ran in dry-run mode.
    DryRun,
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
#[derive(Debug, Default)]
pub struct TaskStats {
    /// Number of items changed or applied.
    pub changed: u32,
    /// Number of items already in the correct state.
    pub already_ok: u32,
    /// Number of items deliberately skipped due to inapplicability.
    pub skipped: u32,
    /// Number of items that failed without aborting the enclosing task.
    pub failed: u32,
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

    /// Log the summary without constructing a [`TaskResult`].
    ///
    /// Only prints to the console when something actually changed, was
    /// skipped, or failed non-fatally. Quiet idempotent runs reduce noise on
    /// no-op invocations.
    pub fn log_summary(&self, ctx: &Context) {
        let msg = self.summary(ctx.dry_run());
        if self.changed > 0 || self.skipped > 0 || self.failed > 0 {
            ctx.log().info(&msg);
        } else {
            ctx.log().debug(&msg);
        }
    }

    /// Convert these counters into the appropriate [`TaskResult`] without
    /// logging.
    pub fn into_result(self, dry_run: bool) -> TaskResult {
        let msg = self.summary(dry_run);
        if self.failed > 0 {
            TaskResult::Failed(msg)
        } else if dry_run && self.changed > 0 {
            TaskResult::DryRun
        } else if self.changed > 0 {
            TaskResult::OkWithMessage(msg)
        } else {
            TaskResult::Ok
        }
    }

    /// Log the summary and return the appropriate [`TaskResult`].
    pub fn finish(self, ctx: &Context) -> TaskResult {
        self.log_summary(ctx);
        let dry_run = ctx.dry_run();
        self.into_result(dry_run)
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
        };
        let s = stats.summary(false);
        assert!(!s.contains("skipped"), "should not mention skipped: {s}");
    }

    // -------------------------------------------------------------------
    // TaskStats::into_result
    // -------------------------------------------------------------------

    #[test]
    fn into_result_returns_dry_run_without_context() {
        let stats = TaskStats {
            changed: 1,
            already_ok: 0,
            skipped: 0,
            failed: 0,
        };

        assert!(matches!(stats.into_result(true), TaskResult::DryRun));
    }

    #[test]
    fn into_result_preserves_failure_summary() {
        let stats = TaskStats {
            changed: 0,
            already_ok: 2,
            skipped: 0,
            failed: 1,
        };

        assert!(
            matches!(
                stats.into_result(false),
                TaskResult::Failed(message) if message == "0 changed, 2 already ok, 1 failed"
            ),
            "failed stats should become a failure with the formatted summary"
        );
    }

    // -------------------------------------------------------------------
    // TaskStats::finish
    // -------------------------------------------------------------------

    #[test]
    fn finish_returns_ok_with_message_when_changes_were_recorded() {
        let config = crate::test_helpers::empty_config(std::path::PathBuf::from("/dotfiles"));
        let ctx = crate::test_helpers::make_linux_context(config);
        let stats = TaskStats {
            changed: 1,
            already_ok: 0,
            skipped: 0,
            failed: 0,
        };
        assert!(matches!(stats.finish(&ctx), TaskResult::OkWithMessage(_)));
    }

    #[test]
    fn finish_returns_ok_when_no_changes_were_recorded() {
        let config = crate::test_helpers::empty_config(std::path::PathBuf::from("/dotfiles"));
        let ctx = crate::test_helpers::make_linux_context(config);
        let stats = TaskStats {
            changed: 0,
            already_ok: 1,
            skipped: 0,
            failed: 0,
        };
        assert!(matches!(stats.finish(&ctx), TaskResult::Ok));
    }

    #[test]
    fn finish_returns_ok_when_only_resource_skips_were_recorded() {
        let config = crate::test_helpers::empty_config(std::path::PathBuf::from("/dotfiles"));
        let ctx = crate::test_helpers::make_linux_context(config);
        let stats = TaskStats {
            changed: 0,
            already_ok: 0,
            skipped: 1,
            failed: 0,
        };
        assert!(matches!(stats.finish(&ctx), TaskResult::Ok));
    }

    #[test]
    fn finish_returns_dry_run_when_dry_run() {
        let config = crate::test_helpers::empty_config(std::path::PathBuf::from("/dotfiles"));
        let ctx = crate::test_helpers::make_linux_context(config).with_dry_run(true);
        let stats = TaskStats {
            changed: 1,
            already_ok: 0,
            skipped: 0,
            failed: 0,
        };
        assert!(matches!(stats.finish(&ctx), TaskResult::DryRun));
    }

    #[test]
    fn finish_returns_ok_when_dry_run_has_no_changes() {
        let config = crate::test_helpers::empty_config(std::path::PathBuf::from("/dotfiles"));
        let ctx = crate::test_helpers::make_linux_context(config).with_dry_run(true);
        let stats = TaskStats {
            changed: 0,
            already_ok: 1,
            skipped: 0,
            failed: 0,
        };
        assert!(matches!(stats.finish(&ctx), TaskResult::Ok));
    }

    #[test]
    fn finish_returns_failed_when_non_fatal_failures_were_recorded() {
        let config = crate::test_helpers::empty_config(std::path::PathBuf::from("/dotfiles"));
        let ctx = crate::test_helpers::make_linux_context(config);
        let stats = TaskStats {
            changed: 0,
            already_ok: 1,
            skipped: 0,
            failed: 1,
        };

        assert!(matches!(stats.finish(&ctx), TaskResult::Failed(_)));
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
        };
        let b = TaskStats {
            changed: 10,
            already_ok: 20,
            skipped: 30,
            failed: 40,
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
    fn task_result_dry_run_matches() {
        assert!(matches!(TaskResult::DryRun, TaskResult::DryRun));
    }

    #[test]
    fn task_result_not_applicable_carries_reason() {
        let r = TaskResult::NotApplicable("no config".into());
        match r {
            TaskResult::NotApplicable(reason) => assert_eq!(reason, "no config"),
            other @ (TaskResult::Ok
            | TaskResult::OkWithMessage(_)
            | TaskResult::Skipped(_)
            | TaskResult::Failed(_)
            | TaskResult::DryRun) => panic!("expected NotApplicable, got {other:?}"),
        }
    }

    #[test]
    fn task_result_skipped_carries_reason() {
        let r = TaskResult::Skipped("wrong platform".into());
        match r {
            TaskResult::Skipped(reason) => assert_eq!(reason, "wrong platform"),
            other @ (TaskResult::Ok
            | TaskResult::OkWithMessage(_)
            | TaskResult::NotApplicable(_)
            | TaskResult::Failed(_)
            | TaskResult::DryRun) => panic!("expected Skipped, got {other:?}"),
        }
    }

    #[test]
    fn task_result_failed_carries_reason() {
        let r = TaskResult::Failed("git pull failed".into());
        match r {
            TaskResult::Failed(reason) => assert_eq!(reason, "git pull failed"),
            other @ (TaskResult::Ok
            | TaskResult::OkWithMessage(_)
            | TaskResult::NotApplicable(_)
            | TaskResult::Skipped(_)
            | TaskResult::DryRun) => panic!("expected Failed, got {other:?}"),
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
