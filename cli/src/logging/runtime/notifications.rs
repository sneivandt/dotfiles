//! Parallel-task lifecycle notifications for [`Logger`].
//!
//! These methods coordinate the in-progress status line as parallel tasks
//! start and complete, ensuring the line stays accurate without overlapping
//! other console output.

use super::{Logger, progress::stdout_supports_progress};

#[allow(
    clippy::print_stderr,
    reason = "intentional user-facing diagnostics for poisoned console lock recovery"
)]
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
        if let Ok(mut active) = self.active_tasks.lock() {
            active.push(name.to_string());
        }
        self.redraw_active_status_locked(show_progress);
    }

    /// Redraw the live result sections and active-task status row.
    ///
    /// Must be called while holding `flush_lock`.
    pub(in crate::logging) fn redraw_status_locked(&self, show_progress: bool) {
        self.clear_progress();
        if !show_progress {
            return;
        }

        let mut lines = self.live_task_section_lines();
        let has_active_status = self.active_task_summary().is_some_and(|names| {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push(format!("\x1b[2m▹ {names}\x1b[0m"));
            true
        });
        self.draw_status_lines(&lines);
        self.set_status_row_visible(has_active_status);
    }

    /// Redraw only the active-task status row when result sections are unchanged.
    ///
    /// Must be called while holding `flush_lock`.
    fn redraw_active_status_locked(&self, show_progress: bool) {
        if !show_progress {
            self.clear_progress();
            return;
        }

        let Some(names) = self.active_task_summary() else {
            self.clear_progress();
            return;
        };
        let line = format!("\x1b[2m▹ {names}\x1b[0m");
        if self.has_status_row() {
            self.replace_status_line(&line);
        } else {
            self.append_status_line(&line, self.has_transient_rows());
        }
    }

    /// Redraw the live result sections and active-task status row.
    pub(crate) fn redraw_status(&self) {
        self.redraw_status_with_progress(stdout_supports_progress());
    }

    /// Redraw the live result sections and active-task status row.
    pub(in crate::logging) fn redraw_status_with_progress(&self, show_progress: bool) {
        let _guard = self.flush_lock.lock().unwrap_or_else(|e| {
            eprintln!("warning: flush lock was poisoned, recovering");
            e.into_inner()
        });
        self.redraw_status_locked(show_progress);
    }

    pub(in crate::logging) fn remove_active_task_locked(&self, name: &str) {
        if let Ok(mut active) = self.active_tasks.lock() {
            active.retain(|n| n != name);
        }
    }

    fn active_task_summary(&self) -> Option<String> {
        self.active_tasks.lock().ok().and_then(|active| {
            if active.is_empty() {
                None
            } else {
                Some(self.format_active(&active))
            }
        })
    }

    /// Record a task completion, optionally redrawing the interactive progress line.
    #[cfg(test)]
    pub(in crate::logging) fn notify_task_done_with_progress(
        &self,
        name: &str,
        show_progress: bool,
    ) {
        let _guard = self.flush_lock.lock().unwrap_or_else(|e| {
            eprintln!("warning: flush lock was poisoned, recovering");
            e.into_inner()
        });
        self.remove_active_task_locked(name);
        self.redraw_status_locked(show_progress);
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
    use crate::logging::{TaskStatus, isolated_logger};

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

    #[test]
    fn notify_task_start_only_replaces_existing_status_row() {
        let (log, _tmp, _guard) = isolated_logger();
        log.record_task("changed-task", TaskStatus::Changed, None);
        log.redraw_status_with_progress(true);
        assert_eq!(log.progress_rows_count(), 2);
        assert!(!log.status_row_visible());

        log.notify_task_start_with_progress("task-a", true);
        assert_eq!(
            log.progress_rows_count(),
            4,
            "status row should be appended below existing result rows with a blank spacer"
        );
        assert!(log.status_row_visible());

        log.notify_task_start_with_progress("task-b", true);
        assert_eq!(
            log.progress_rows_count(),
            4,
            "adding another active task should replace only the existing status row"
        );
        assert!(log.status_row_visible());
    }
}
