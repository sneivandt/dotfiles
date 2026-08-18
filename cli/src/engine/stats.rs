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
/// let skipped = TaskResult::skipped("not on arch");
/// let failed = TaskResult::Failed("git pull failed".into());
///
/// assert!(matches!(ok, TaskResult::Ok));
/// assert!(matches!(changed, TaskResult::Batch(_)));
/// assert!(matches!(na, TaskResult::NotApplicable(_)));
/// assert!(matches!(skipped, TaskResult::Skipped { .. }));
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
    Skipped {
        /// Human-readable explanation shown on the task status row.
        reason: String,
        /// Whether the skip is harmless or leaves desired work incomplete.
        kind: crate::engine::SkipKind,
    },
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

impl TaskResult {
    /// Deliberately skip work without making the run incomplete.
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self::Skipped {
            reason: reason.into(),
            kind: crate::engine::SkipKind::Benign,
        }
    }

    /// Skip applicable work that could not be converged.
    pub fn unmet(reason: impl Into<String>) -> Self {
        Self::Skipped {
            reason: reason.into(),
            kind: crate::engine::SkipKind::UnmetWork,
        }
    }
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
/// let stats = TaskStats::from_counts(3, 10, 0, 0);
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
/// let stats = TaskStats::from_counts(1, 2, 3, 1);
/// assert_eq!(stats.summary(false), "1 changed, 2 already ok, 3 skipped, 1 failed");
/// ```
#[derive(Debug, Clone, Default)]
pub struct TaskStats {
    /// Number of items changed or applied.
    #[cfg(not(test))]
    changed: u32,
    #[cfg(test)]
    pub(super) changed: u32,
    /// Number of items already in the correct state.
    #[cfg(not(test))]
    already_ok: u32,
    #[cfg(test)]
    pub(super) already_ok: u32,
    /// Number of items deliberately skipped due to inapplicability.
    #[cfg(not(test))]
    skipped: u32,
    #[cfg(test)]
    pub(super) skipped: u32,
    /// Number of items that failed without aborting the enclosing task.
    #[cfg(not(test))]
    failed: u32,
    #[cfg(test)]
    pub(super) failed: u32,
    /// Optional domain-specific summary for this batch.
    #[cfg(not(test))]
    message: Option<String>,
    #[cfg(test)]
    pub(super) message: Option<String>,
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
    /// assert_eq!(stats.changed_count(), 0);
    /// assert_eq!(stats.already_ok_count(), 0);
    /// assert_eq!(stats.skipped_count(), 0);
    /// assert_eq!(stats.failed_count(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a complete counter set while preserving the type's invariants.
    #[must_use]
    pub const fn from_counts(changed: u32, already_ok: u32, skipped: u32, failed: u32) -> Self {
        Self {
            changed,
            already_ok,
            skipped,
            failed,
            message: None,
        }
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

    /// Attach a domain-specific summary message.
    #[must_use]
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Number of changed items.
    #[must_use]
    pub const fn changed_count(&self) -> u32 {
        self.changed
    }

    /// Number of already-correct items.
    #[must_use]
    pub const fn already_ok_count(&self) -> u32 {
        self.already_ok
    }

    /// Number of deliberately skipped items.
    #[must_use]
    pub const fn skipped_count(&self) -> u32 {
        self.skipped
    }

    /// Number of failed items.
    #[must_use]
    pub const fn failed_count(&self) -> u32 {
        self.failed
    }

    /// Optional domain-specific summary message.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
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
    /// let stats = TaskStats::from_counts(5, 12, 0, 0);
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
