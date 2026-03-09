//! Result and statistics types for task execution.

use super::context::Context;

/// Result of a single task execution.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::tasks::TaskResult;
///
/// let ok = TaskResult::Ok;
/// let na = TaskResult::NotApplicable("nothing configured".into());
/// let skipped = TaskResult::Skipped("not on arch".into());
/// let failed = TaskResult::Failed("git pull failed".into());
/// let dry = TaskResult::DryRun;
///
/// assert!(matches!(ok, TaskResult::Ok));
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
/// use dotfiles_cli::tasks::TaskStats;
///
/// let mut stats = TaskStats::new();
/// stats.changed = 3;
/// stats.already_ok = 10;
///
/// assert_eq!(stats.summary(false), "3 changed, 10 already ok");
/// assert_eq!(stats.summary(true), "3 would change, 10 already ok");
/// ```
///
/// When items are skipped, the summary includes the count:
///
/// ```
/// use dotfiles_cli::tasks::TaskStats;
///
/// let stats = TaskStats { changed: 1, already_ok: 2, skipped: 3 };
/// assert_eq!(stats.summary(false), "1 changed, 2 already ok, 3 skipped");
/// ```
#[derive(Debug, Default)]
pub struct TaskStats {
    /// Number of items changed or applied.
    pub changed: u32,
    /// Number of items already in the correct state.
    pub already_ok: u32,
    /// Number of items skipped due to errors or inapplicability.
    pub skipped: u32,
}

impl TaskStats {
    /// Create a new empty stats counter.
    ///
    /// # Examples
    ///
    /// ```
    /// use dotfiles_cli::tasks::TaskStats;
    ///
    /// let stats = TaskStats::new();
    /// assert_eq!(stats.changed, 0);
    /// assert_eq!(stats.already_ok, 0);
    /// assert_eq!(stats.skipped, 0);
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
    /// use dotfiles_cli::tasks::TaskStats;
    ///
    /// let stats = TaskStats { changed: 5, already_ok: 12, skipped: 0 };
    /// assert_eq!(stats.summary(false), "5 changed, 12 already ok");
    /// assert_eq!(stats.summary(true), "5 would change, 12 already ok");
    /// ```
    #[must_use]
    pub fn summary(&self, dry_run: bool) -> String {
        let verb = if dry_run { "would change" } else { "changed" };
        if self.skipped > 0 {
            format!(
                "{} {verb}, {} already ok, {} skipped",
                self.changed, self.already_ok, self.skipped
            )
        } else {
            format!("{} {verb}, {} already ok", self.changed, self.already_ok)
        }
    }

    /// Log the summary and return the appropriate `TaskResult`.
    pub fn finish(self, ctx: &Context) -> TaskResult {
        ctx.log.info(&self.summary(ctx.dry_run));
        if ctx.dry_run {
            TaskResult::DryRun
        } else {
            TaskResult::Ok
        }
    }
}

impl std::ops::AddAssign for TaskStats {
    fn add_assign(&mut self, other: Self) {
        self.changed += other.changed;
        self.already_ok += other.already_ok;
        self.skipped += other.skipped;
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
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
    }

    #[test]
    fn default_stats_are_all_zero() {
        let stats = TaskStats::default();
        assert_eq!(stats.changed, 0);
        assert_eq!(stats.already_ok, 0);
        assert_eq!(stats.skipped, 0);
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
        };
        assert_eq!(stats.summary(false), "5 changed, 0 already ok");
    }

    #[test]
    fn summary_already_ok_only() {
        let stats = TaskStats {
            changed: 0,
            already_ok: 10,
            skipped: 0,
        };
        assert_eq!(stats.summary(false), "0 changed, 10 already ok");
    }

    #[test]
    fn summary_with_skipped() {
        let stats = TaskStats {
            changed: 1,
            already_ok: 2,
            skipped: 3,
        };
        assert_eq!(stats.summary(false), "1 changed, 2 already ok, 3 skipped");
    }

    #[test]
    fn summary_with_skipped_dry_run() {
        let stats = TaskStats {
            changed: 1,
            already_ok: 2,
            skipped: 3,
        };
        assert_eq!(
            stats.summary(true),
            "1 would change, 2 already ok, 3 skipped"
        );
    }

    #[test]
    fn summary_hides_skipped_when_zero() {
        let stats = TaskStats {
            changed: 3,
            already_ok: 7,
            skipped: 0,
        };
        let s = stats.summary(false);
        assert!(!s.contains("skipped"), "should not mention skipped: {s}");
    }

    // -------------------------------------------------------------------
    // TaskStats::finish
    // -------------------------------------------------------------------

    #[test]
    fn finish_returns_ok_when_not_dry_run() {
        let config =
            crate::tasks::test_helpers::empty_config(std::path::PathBuf::from("/dotfiles"));
        let ctx = crate::tasks::test_helpers::make_linux_context(config);
        let stats = TaskStats {
            changed: 1,
            already_ok: 0,
            skipped: 0,
        };
        assert!(matches!(stats.finish(&ctx), TaskResult::Ok));
    }

    #[test]
    fn finish_returns_dry_run_when_dry_run() {
        let config =
            crate::tasks::test_helpers::empty_config(std::path::PathBuf::from("/dotfiles"));
        let ctx = crate::tasks::test_helpers::make_linux_context(config).with_dry_run(true);
        let stats = TaskStats {
            changed: 1,
            already_ok: 0,
            skipped: 0,
        };
        assert!(matches!(stats.finish(&ctx), TaskResult::DryRun));
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
        };
        let b = TaskStats {
            changed: 10,
            already_ok: 20,
            skipped: 30,
        };
        a += b;
        assert_eq!(a.changed, 11);
        assert_eq!(a.already_ok, 22);
        assert_eq!(a.skipped, 33);
    }

    #[test]
    fn add_assign_with_zero_is_identity() {
        let mut a = TaskStats {
            changed: 5,
            already_ok: 3,
            skipped: 1,
        };
        a += TaskStats::new();
        assert_eq!(a.changed, 5);
        assert_eq!(a.already_ok, 3);
        assert_eq!(a.skipped, 1);
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
            other => panic!("expected NotApplicable, got {other:?}"),
        }
    }

    #[test]
    fn task_result_skipped_carries_reason() {
        let r = TaskResult::Skipped("wrong platform".into());
        match r {
            TaskResult::Skipped(reason) => assert_eq!(reason, "wrong platform"),
            other => panic!("expected Skipped, got {other:?}"),
        }
    }

    #[test]
    fn task_result_failed_carries_reason() {
        let r = TaskResult::Failed("git pull failed".into());
        match r {
            TaskResult::Failed(reason) => assert_eq!(reason, "git pull failed"),
            other => panic!("expected Failed, got {other:?}"),
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
        #[allow(clippy::redundant_clone)]
        let r2 = r.clone();
        assert!(matches!(r2, TaskResult::Skipped(_)));
    }
}
