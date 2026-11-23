//! Parallel-task lifecycle notifications for [`Logger`].
//!
//! These methods coordinate the in-progress status line as parallel tasks start
//! and complete, ensuring active-task updates never overlap other console output.

use super::Logger;

impl Logger {
    /// Record that a parallel task has started.
    ///
    /// Acquires the flush lock, erases any previous progress line, adds the
    /// task to the active set, and redraws the status line.
    pub fn notify_task_start(&self, name: &str) {
        self.notify_task_start_with_progress(name, self.progress_terminal);
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
        let show_progress = self.progress_terminal;
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
    fn a_completed_task_emits_its_result_and_restores_other_active_work() {
        let (mut log, _tmp, _guard) = isolated_logger();
        log.progress_terminal = true;
        log.notify_task_start("Still running");
        log.record_task(task_entry(
            "Finished fixture",
            TaskStatus::Changed,
            TaskVisibility::Visible,
        ));
        log.emit_task_result_and_redraw("Finished fixture");
        let output = log.captured_output();
        assert!(
            output
                .iter()
                .any(|(_, line)| line.contains("Finished fixture")),
            "{output:?}"
        );
        assert!(
            output.last().unwrap().1.contains("Still running"),
            "{output:?}"
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
    fn task_lifetimes_occupy_one_status_row_until_the_last_finishes() {
        for names in [
            &["single"][..],
            &["task-a", "task-b"],
            &["shared", "shared"],
        ] {
            for interactive in [false, true] {
                let (log, _tmp, _guard) = isolated_logger();
                let assert_active = |expected: &[&str]| {
                    let visible = interactive && !expected.is_empty();
                    assert_eq!(&*log.lock_active_tasks(), expected, "{names:?}");
                    assert_eq!(
                        log.status_row_visible(),
                        visible,
                        "{expected:?}, interactive={interactive}"
                    );
                    assert_eq!(log.progress_rows_count(), u16::from(visible));
                };
                assert_active(&[]);
                assert!(!log.console.lock().startup_separator_emitted);
                for (index, name) in names.iter().enumerate() {
                    log.notify_task_start_with_progress(name, interactive);
                    assert_active(&names[..=index]);
                }
                if interactive {
                    assert!(log.console.lock().startup_separator_emitted);
                }
                for (index, name) in names.iter().enumerate() {
                    log.notify_task_done_with_progress(name, interactive);
                    assert_active(&names[index + 1..]);
                }
            }
        }
    }

    #[test]
    fn status_spacing_reuses_a_blank_line_but_separates_durable_output() {
        use crate::infra::logging::Logger;

        let check = |expected_rows, emit: fn(&Logger)| {
            let (log, _tmp, _guard) = isolated_logger();
            emit(&log);
            log.notify_task_start_with_progress("task-a", true);
            assert!(log.status_row_visible());
            assert_eq!(log.progress_rows_count(), expected_rows);
        };
        check(1, |log| {
            log.always("prior output");
            log.always("");
        });
        check(2, |log| {
            log.separate_from_startup();
            log.warn("warning after startup");
        });
        check(2, |log| {
            log.separate_from_startup();
            log.console.task_result("completed task");
            log.mark_task_console_output();
            assert!(log.task_console_output_emitted());
        });
    }
}
