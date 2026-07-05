//! Parallel-task lifecycle notifications for [`Logger`].
//!
//! These methods coordinate the in-progress status line as parallel tasks
//! start and complete, ensuring the line stays accurate without overlapping
//! other console output.

use super::{Logger, progress::stdout_supports_progress};

#[allow(clippy::print_stderr, reason = "intentional user-facing output")]
impl Logger {
    /// Record that a parallel task has started.
    ///
    /// Acquires the flush lock, erases any previous progress line, adds the
    /// task to the active set, and redraws the status line.
    pub fn notify_task_start(&self, name: &str) {
        self.notify_task_start_with_progress(name, stdout_supports_progress());
    }

    /// Record a task start, optionally drawing the interactive progress line.
    pub(in crate::logging) fn notify_task_start_with_progress(
        &self,
        name: &str,
        show_progress: bool,
    ) {
        let _guard = self.flush_lock.lock().unwrap_or_else(|e| {
            eprintln!("warning: flush lock was poisoned, recovering");
            e.into_inner()
        });
        if show_progress {
            self.clear_progress();
        }
        let names = self.active_tasks.lock().map_or_else(
            |_| name.to_string(),
            |mut active| {
                active.push(name.to_string());
                self.format_active(&active)
            },
        );
        if show_progress {
            self.draw_progress(&names);
        }
    }

    /// Record that a parallel task has completed (successfully or otherwise).
    ///
    /// Acquires the flush lock, removes `name` from the active set, and
    /// redraws the progress line with the remaining tasks.  If no tasks
    /// remain active the progress line is cleared and `progress_rows` is
    /// set to `0`.
    ///
    /// Must be called while **not** already holding `flush_lock` to avoid
    /// deadlocking.
    pub fn notify_task_done(&self, name: &str) {
        self.notify_task_done_with_progress(name, stdout_supports_progress());
    }

    /// Record a task completion, optionally redrawing the interactive progress line.
    pub(in crate::logging) fn notify_task_done_with_progress(
        &self,
        name: &str,
        show_progress: bool,
    ) {
        let _guard = self.flush_lock.lock().unwrap_or_else(|e| {
            eprintln!("warning: flush lock was poisoned, recovering");
            e.into_inner()
        });
        if show_progress {
            self.clear_progress();
        }
        let remaining = self.active_tasks.lock().ok().and_then(|mut active| {
            active.retain(|n| n != name);
            if active.is_empty() {
                None
            } else {
                Some(self.format_active(&active))
            }
        });
        if let Some(names) = remaining
            && show_progress
        {
            self.draw_progress(&names);
        }
    }

    /// Build the progress-line text describing the currently active tasks.
    ///
    /// In verbose mode every task name is listed. Otherwise the first three
    /// active task names are shown, followed by a count of any remaining tasks.
    fn format_active(&self, active: &[String]) -> String {
        if self.verbose {
            return active.join(", ");
        }
        let mut names = active
            .iter()
            .take(3)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        let remaining = active.len().saturating_sub(3);
        if remaining > 0 {
            names.push_str(", +");
            names.push_str(&remaining.to_string());
            names.push_str(" more");
        }
        format!("{names} \u{2026}")
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use crate::logging::isolated_logger;

    #[test]
    #[allow(clippy::significant_drop_tightening, reason = "intentional lock scope")]
    fn notify_task_start_adds_to_active_tasks() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start_with_progress("my-task", false);
        let active = log.active_tasks.lock().unwrap();
        assert!(
            active.contains(&"my-task".to_string()),
            "active_tasks should contain 'my-task'"
        );
    }

    #[test]
    fn format_active_names_single_task() {
        let (mut log, _tmp, _guard) = isolated_logger();
        log.verbose = false;
        assert_eq!(
            log.format_active(&["only-task".to_string()]),
            "only-task \u{2026}",
            "a single active task should be named directly"
        );
    }

    #[test]
    fn format_active_names_multiple_tasks() {
        let (mut log, _tmp, _guard) = isolated_logger();
        log.verbose = false;
        assert_eq!(
            log.format_active(&["task-a".to_string(), "task-b".to_string()]),
            "task-a, task-b \u{2026}",
            "multiple active tasks should show task names"
        );
    }

    #[test]
    fn format_active_names_first_three_then_remaining_count() {
        let (mut log, _tmp, _guard) = isolated_logger();
        log.verbose = false;
        assert_eq!(
            log.format_active(&[
                "task-a".to_string(),
                "task-b".to_string(),
                "task-c".to_string(),
                "task-d".to_string(),
                "task-e".to_string()
            ]),
            "task-a, task-b, task-c, +2 more \u{2026}",
            "more than three active tasks should show first names plus overflow count"
        );
    }

    #[test]
    fn notify_task_start_sets_progress_rows_to_one() {
        let (log, _tmp, _guard) = isolated_logger();
        assert_eq!(log.progress_rows_count(), 0, "progress_rows starts at 0");
        log.notify_task_start_with_progress("task-a", true);
        assert_eq!(
            log.progress_rows_count(),
            1,
            "progress_rows should be 1 after first notify_task_start"
        );
    }

    #[test]
    #[allow(clippy::significant_drop_tightening, reason = "intentional lock scope")]
    fn notify_task_done_removes_from_active_tasks() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start_with_progress("my-task", false);
        log.notify_task_done_with_progress("my-task", false);
        let active = log.active_tasks.lock().unwrap();
        assert!(
            !active.contains(&"my-task".to_string()),
            "active_tasks should not contain 'my-task' after notify_task_done"
        );
    }

    #[test]
    fn notify_task_done_clears_progress_when_last_task_completes() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start_with_progress("task-a", true);
        assert_eq!(
            log.progress_rows_count(),
            1,
            "progress_rows should be 1 after start"
        );
        log.notify_task_done_with_progress("task-a", true);
        assert_eq!(
            log.progress_rows_count(),
            0,
            "progress_rows should be 0 after last task completes"
        );
    }

    #[test]
    fn notify_task_done_keeps_progress_when_tasks_remain() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start_with_progress("task-a", true);
        log.notify_task_start_with_progress("task-b", true);
        log.notify_task_done_with_progress("task-a", true);
        assert_eq!(
            log.progress_rows_count(),
            1,
            "progress_rows should still be 1 when task-b is still active"
        );
    }

    #[test]
    #[allow(clippy::significant_drop_tightening, reason = "intentional lock scope")]
    fn notify_task_done_multiple_tasks_all_complete() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start_with_progress("task-a", true);
        log.notify_task_start_with_progress("task-b", true);
        log.notify_task_done_with_progress("task-a", true);
        {
            let active = log.active_tasks.lock().unwrap();
            assert!(
                !active.contains(&"task-a".to_string()),
                "task-a should be removed"
            );
            assert!(
                active.contains(&"task-b".to_string()),
                "task-b should still be present"
            );
        }
        log.notify_task_done_with_progress("task-b", true);
        {
            let active = log.active_tasks.lock().unwrap();
            assert!(
                active.is_empty(),
                "active_tasks should be empty after both tasks complete"
            );
        }
        assert_eq!(
            log.progress_rows_count(),
            0,
            "progress_rows should be 0 after all tasks complete"
        );
    }

    #[test]
    fn notify_task_start_suppresses_progress_when_not_interactive() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start_with_progress("task-a", false);
        assert_eq!(
            log.progress_rows_count(),
            0,
            "progress_rows should stay zero when progress rendering is disabled"
        );
    }
}
