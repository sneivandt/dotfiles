//! Result and statistics types for task execution.

/// Outcome of processing one item in a batch task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemOutcome {
    Changed,
    AlreadyOk,
    Skipped,
    Failed,
}

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
    /// Task identified work for a dry run, but cannot quantify concrete changes.
    DryRun,
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

    /// Record one item outcome, saturating its counter.
    pub(crate) const fn record(&mut self, outcome: ItemOutcome) {
        match outcome {
            ItemOutcome::Changed => self.changed = self.changed.saturating_add(1),
            ItemOutcome::AlreadyOk => self.already_ok = self.already_ok.saturating_add(1),
            ItemOutcome::Skipped => self.skipped = self.skipped.saturating_add(1),
            ItemOutcome::Failed => self.failed = self.failed.saturating_add(1),
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
