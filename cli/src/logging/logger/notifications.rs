//! Parallel-task lifecycle notifications for [`Logger`].
//!
//! These methods coordinate the in-progress status line as parallel tasks
//! start and complete, ensuring the line stays accurate without overlapping
//! other console output.

use super::Logger;

#[allow(clippy::print_stderr)]
impl Logger {
    /// Record that a parallel task has started.
    ///
    /// Acquires the flush lock, erases any previous progress line, adds the
    /// task to the active set, and redraws the status line.
    pub fn notify_task_start(&self, name: &str) {
        let _guard = self.flush_lock.lock().unwrap_or_else(|e| {
            eprintln!("warning: flush lock was poisoned, recovering");
            e.into_inner()
        });
        self.clear_progress();
        let names = self.active_tasks.lock().map_or_else(
            |_| name.to_string(),
            |mut active| {
                active.push(name.to_string());
                if self.verbose {
                    active.join(", ")
                } else {
                    format!("{} tasks running\u{2026}", active.len())
                }
            },
        );
        self.draw_progress(&names);
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
        let _guard = self.flush_lock.lock().unwrap_or_else(|e| {
            eprintln!("warning: flush lock was poisoned, recovering");
            e.into_inner()
        });
        self.clear_progress();
        let remaining = self.active_tasks.lock().ok().and_then(|mut active| {
            active.retain(|n| n != name);
            if active.is_empty() {
                None
            } else if self.verbose {
                Some(active.join(", "))
            } else {
                Some(format!("{} tasks running\u{2026}", active.len()))
            }
        });
        if let Some(names) = remaining {
            self.draw_progress(&names);
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use crate::logging::isolated_logger;

    #[test]
    #[allow(clippy::significant_drop_tightening)]
    fn notify_task_start_adds_to_active_tasks() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start("my-task");
        let active = log.active_tasks.lock().unwrap();
        assert!(
            active.contains(&"my-task".to_string()),
            "active_tasks should contain 'my-task'"
        );
    }

    #[test]
    fn notify_task_start_sets_progress_rows_to_one() {
        let (log, _tmp, _guard) = isolated_logger();
        assert_eq!(log.progress_rows_count(), 0, "progress_rows starts at 0");
        log.notify_task_start("task-a");
        assert_eq!(
            log.progress_rows_count(),
            1,
            "progress_rows should be 1 after first notify_task_start"
        );
    }

    #[test]
    #[allow(clippy::significant_drop_tightening)]
    fn notify_task_done_removes_from_active_tasks() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start("my-task");
        log.notify_task_done("my-task");
        let active = log.active_tasks.lock().unwrap();
        assert!(
            !active.contains(&"my-task".to_string()),
            "active_tasks should not contain 'my-task' after notify_task_done"
        );
    }

    #[test]
    fn notify_task_done_clears_progress_when_last_task_completes() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start("task-a");
        assert_eq!(
            log.progress_rows_count(),
            1,
            "progress_rows should be 1 after start"
        );
        log.notify_task_done("task-a");
        assert_eq!(
            log.progress_rows_count(),
            0,
            "progress_rows should be 0 after last task completes"
        );
    }

    #[test]
    fn notify_task_done_keeps_progress_when_tasks_remain() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start("task-a");
        log.notify_task_start("task-b");
        log.notify_task_done("task-a");
        assert_eq!(
            log.progress_rows_count(),
            1,
            "progress_rows should still be 1 when task-b is still active"
        );
    }

    #[test]
    #[allow(clippy::significant_drop_tightening)]
    fn notify_task_done_multiple_tasks_all_complete() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start("task-a");
        log.notify_task_start("task-b");
        log.notify_task_done("task-a");
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
        log.notify_task_done("task-b");
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
}
