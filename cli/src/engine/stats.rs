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
/// let skipped = TaskResult::Skipped("not on arch".into());
/// let dry = TaskResult::DryRun;
///
/// assert!(matches!(ok, TaskResult::Ok));
/// assert!(matches!(skipped, TaskResult::Skipped(_)));
/// assert!(matches!(dry, TaskResult::DryRun));
/// ```
#[derive(Debug, Clone)]
#[must_use]
pub enum TaskResult {
    /// Task completed successfully.
    Ok,
    /// Task was skipped (not applicable to this platform/profile).
    Skipped(String),
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
