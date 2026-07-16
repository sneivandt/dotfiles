//! Parallel-task lifecycle notifications for [`Logger`].
//!
//! These methods coordinate the in-progress status line as parallel tasks start
//! and complete, ensuring active-task updates never overlap other console output.

use super::{Logger, progress::stdout_supports_progress};
use crate::infra::logging::style::{TextStyle, stdout_style};

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
    pub(in crate::infra::logging) fn notify_task_start_with_progress(
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

    /// Redraw only the active-task status row.
    ///
    /// Must be called while holding `flush_lock`.
    pub(in crate::infra::logging) fn redraw_active_status_locked(&self, show_progress: bool) {
        if !show_progress {
            self.clear_progress();
            return;
        }

        let Some(names) = self.active_task_summary() else {
            self.clear_progress();
            return;
        };
        let line = format!(
            "Running {}",
            stdout_style().paint(TextStyle::Dim, &format!("\u{00b7} {names}"))
        );
        if self.has_status_row() {
            self.replace_status_line(&line);
        } else {
            self.append_status_line(
                &line,
                self.has_transient_rows() || self.has_task_console_output(),
            );
        }
    }

    /// Emit a completed task result and then redraw the active-task status row.
    pub(crate) fn emit_task_result_and_redraw(&self, task_name: &str) {
        let show_progress = stdout_supports_progress();
        let _guard = self.flush_lock.lock().unwrap_or_else(|e| {
            eprintln!("warning: flush lock was poisoned, recovering");
            e.into_inner()
        });
        if show_progress {
            self.clear_progress();
        }
        if !self.is_verbose() {
            self.emit_recorded_task_result(task_name);
        }
        self.redraw_active_status_locked(show_progress);
    }

    pub(in crate::infra::logging) fn remove_active_task_locked(&self, name: &str) {
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
    pub(in crate::infra::logging) fn notify_task_done_with_progress(
        &self,
        name: &str,
        show_progress: bool,
    ) {
        let _guard = self.flush_lock.lock().unwrap_or_else(|e| {
            eprintln!("warning: flush lock was poisoned, recovering");
            e.into_inner()
        });
        self.remove_active_task_locked(name);
        self.clear_progress();
        self.redraw_active_status_locked(show_progress);
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
        names
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use crate::infra::logging::isolated_logger;

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
            "only-task",
            "a single active task should be named directly"
        );
    }

    #[test]
    fn format_active_names_multiple_tasks() {
        let (mut log, _tmp, _guard) = isolated_logger();
        log.verbose = false;
        assert_eq!(
            log.format_active(&["task-a".to_string(), "task-b".to_string()]),
            "task-a, task-b",
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
            "task-a, task-b, task-c, +2 more",
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
        log.notify_task_start_with_progress("task-a", true);
        assert_eq!(
            log.progress_rows_count(),
            1,
            "first active task should draw one status row"
        );
        assert!(log.status_row_visible());

        log.notify_task_start_with_progress("task-b", true);
        assert_eq!(
            log.progress_rows_count(),
            1,
            "adding another active task should replace only the existing status row"
        );
        assert!(log.status_row_visible());
    }

    #[test]
    fn notify_task_start_adds_blank_row_after_task_console_output() {
        let (log, _tmp, _guard) = isolated_logger();
        log.mark_task_console_output();
        assert!(log.task_console_output_emitted());
        log.notify_task_start_with_progress("task-a", true);
        assert_eq!(
            log.progress_rows_count(),
            2,
            "status row should include a transient blank spacer after task output"
        );
        assert!(log.status_row_visible());
    }
}
