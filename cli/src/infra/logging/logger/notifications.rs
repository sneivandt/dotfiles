//! Parallel-task lifecycle notifications for [`Logger`].
//!
//! These methods coordinate the in-progress status line as parallel tasks start
//! and complete, ensuring active-task updates never overlap other console output.

use super::{Logger, progress::stdout_supports_progress};

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
        let _guard = self.lock_flush();
        self.lock_active_tasks().push(name.to_string());
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

        let active = self.lock_active_tasks();
        if active.is_empty() {
            self.clear_progress();
        } else {
            self.draw_active_status(&active);
        }
    }

    /// Emit a completed task result selected by scheduler identity.
    pub(crate) fn emit_task_result_and_redraw(&self, task_id: &str) {
        let show_progress = stdout_supports_progress();
        let _guard = self.lock_flush();
        if show_progress {
            self.clear_progress();
        }
        self.emit_recorded_task_result(task_id, &[]);
        self.redraw_active_status_locked(show_progress);
    }

    pub(in crate::infra::logging) fn remove_active_task_locked(&self, name: &str) {
        let mut active = self.lock_active_tasks();
        // Dynamic task instances can share a display name.
        if let Some(index) = active.iter().position(|active_name| active_name == name) {
            active.remove(index);
        }
    }

    /// Record a task completion, optionally redrawing the interactive progress line.
    #[cfg(test)]
    pub(in crate::infra::logging) fn notify_task_done_with_progress(
        &self,
        name: &str,
        show_progress: bool,
    ) {
        let _guard = self.lock_flush();
        self.remove_active_task_locked(name);
        self.clear_progress();
        self.redraw_active_status_locked(show_progress);
    }
}

#[cfg(test)]
mod tests {

    use crate::infra::logging::types::{ActionCounts, TaskEntry, TaskStatus, TaskVisibility};
    use crate::infra::logging::{OutputExt as _, isolated_logger};

    fn task_entry(name: &str, status: TaskStatus, visibility: TaskVisibility) -> TaskEntry {
        TaskEntry::new(
            name,
            name,
            status,
            None,
            ActionCounts::default(),
            visibility,
        )
    }

    #[test]
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
    fn task_totals_accumulate_across_scheduled_graphs() {
        let (log, _tmp, _guard) = isolated_logger();
        log.add_task_total(20);
        log.record_task(task_entry("task", TaskStatus::Ok, TaskVisibility::Visible));
        for _ in 0..20 {
            log.mark_task_completed("task");
        }
        log.add_task_total(3);

        assert_eq!(
            log.task_progress(),
            Some((20, 23)),
            "a second graph must extend the denominator instead of replacing it"
        );
    }

    #[test]
    fn task_progress_never_exceeds_the_total() {
        let (log, _tmp, _guard) = isolated_logger();
        log.add_task_total(2);
        log.record_task(task_entry("task", TaskStatus::Ok, TaskVisibility::Visible));
        for _ in 0..5 {
            log.mark_task_completed("task");
        }

        assert_eq!(log.task_progress(), Some((2, 2)));
    }

    #[test]
    fn internal_tasks_are_left_out_of_the_progress_counter() {
        let (log, _tmp, _guard) = isolated_logger();
        log.add_task_total(2);
        log.record_task(task_entry(
            "internal-task",
            TaskStatus::Ok,
            TaskVisibility::Internal,
        ));
        log.mark_task_completed("internal-task");

        assert_eq!(
            log.task_progress(),
            Some((0, 2)),
            "internal tasks never enter the denominator, so finishing one changes nothing"
        );
    }

    #[test]
    fn non_applicable_tasks_complete_without_changing_the_denominator() {
        let (log, _tmp, _guard) = isolated_logger();
        log.add_task_total(3);
        log.record_task(task_entry(
            "applied-task",
            TaskStatus::Changed,
            TaskVisibility::Visible,
        ));
        log.mark_task_completed("applied-task");
        log.record_task(task_entry(
            "elsewhere-task",
            TaskStatus::NotApplicable,
            TaskVisibility::Visible,
        ));
        log.mark_task_completed("elsewhere-task");

        assert_eq!(
            log.task_progress(),
            Some((2, 3)),
            "a completed non-applicable task should advance progress without shrinking the total"
        );
        assert_eq!(
            log.task_entries()
                .last()
                .expect("non-applicable task should remain recorded")
                .status,
            TaskStatus::NotApplicable,
            "progress accounting must not change the reported task result"
        );
    }

    #[test]
    fn notify_task_start_separates_the_first_progress_row_from_startup() {
        let (log, _tmp, _guard) = isolated_logger();
        assert_eq!(log.progress_rows_count(), 0, "progress_rows starts at 0");
        assert!(!log.console.lock().startup_separator_emitted);

        log.notify_task_start_with_progress("task-a", true);

        assert_eq!(
            log.progress_rows_count(),
            1,
            "the durable separator should not count as a transient progress row"
        );
        assert!(
            log.console.lock().startup_separator_emitted,
            "the first running row should be separated from startup output"
        );
    }

    #[test]
    fn notify_task_start_adds_one_spacer_after_later_console_output() {
        let (log, _tmp, _guard) = isolated_logger();
        log.separate_from_startup();
        log.warn("warning after startup");

        log.notify_task_start_with_progress("task-a", true);

        assert_eq!(
            log.progress_rows_count(),
            2,
            "the status and its added blank spacer should both be transient"
        );
    }

    #[test]
    fn notify_task_start_reuses_an_existing_blank_line() {
        let (log, _tmp, _guard) = isolated_logger();
        log.always("prior output");
        log.always("");

        log.notify_task_start_with_progress("task-a", true);

        assert_eq!(
            log.progress_rows_count(),
            1,
            "an existing blank line should not be duplicated or made transient"
        );
    }

    #[test]
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
    fn completing_one_task_keeps_another_with_the_same_display_name_active() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start_with_progress("shared name", true);
        log.notify_task_start_with_progress("shared name", true);

        log.notify_task_done_with_progress("shared name", true);

        assert_eq!(&*log.lock_active_tasks(), &["shared name"]);
        assert_eq!(
            log.progress_rows_count(),
            1,
            "the other same-name task is still running"
        );

        log.notify_task_done_with_progress("shared name", true);

        assert!(log.lock_active_tasks().is_empty());
        assert_eq!(log.progress_rows_count(), 0);
    }

    #[test]
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
        log.separate_from_startup();
        log.task_result("completed task");
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
