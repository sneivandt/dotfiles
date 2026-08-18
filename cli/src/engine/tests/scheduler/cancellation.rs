//! Cooperative cancellation and its interaction with failures.

use super::*;

#[test]
fn scheduler_records_pre_cancelled_tasks_without_running_them() {
    for parallel in [false, true] {
        let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
        let root_ran = Arc::new(AtomicBool::new(false));
        let dependent_ran = Arc::new(AtomicBool::new(false));
        let root = FlagTask {
            ran: Arc::clone(&root_ran),
        };
        let dependent = DepOnFlagTask {
            ran: Arc::clone(&dependent_ran),
        };
        let tasks: Vec<&dyn Task> = vec![&root, &dependent];
        ctx.cancellation_token().cancel();

        run_test_tasks_with_mode(&tasks, &ctx, &log, parallel);

        assert!(!root_ran.load(Ordering::SeqCst));
        assert!(!dependent_ran.load(Ordering::SeqCst));
        let entries = log.task_entries();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| {
            entry.status == TaskStatus::Skipped && entry.message.as_deref() == Some("cancelled")
        }));
    }
}

#[test]
fn cancellation_after_a_task_starts_skips_its_dependents() {
    for parallel in [false, true] {
        let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
        let dependent_ran = Arc::new(AtomicBool::new(false));
        let root = CancellingTask;
        let dependent = DepOnCancellingTask {
            ran: Arc::clone(&dependent_ran),
        };
        let tasks: Vec<&dyn Task> = vec![&root, &dependent];

        run_test_tasks_with_mode(&tasks, &ctx, &log, parallel);

        assert!(!dependent_ran.load(Ordering::SeqCst));
        let entries = log.task_entries();
        let dependent_entry = entries
            .iter()
            .find(|entry| entry.name == "dep-on-cancelling")
            .expect("dependent should be recorded");
        assert_eq!(dependent_entry.status, TaskStatus::Skipped);
        assert_eq!(dependent_entry.message.as_deref(), Some("cancelled"));
    }
}

#[test]
fn dependency_failure_takes_precedence_over_cancellation() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let dependent_ran = Arc::new(AtomicBool::new(false));
    let failed = FailedTask;
    let cancel = CancelAfterFailureTask {
        log: Arc::clone(&log),
    };
    let dependent = DepOnFailureAndCancelTask {
        ran: Arc::clone(&dependent_ran),
    };
    let tasks: Vec<&dyn Task> = vec![&failed, &cancel, &dependent];

    run_test_tasks(&tasks, &ctx, &log);

    assert!(!dependent_ran.load(Ordering::SeqCst));
    let entries = log.task_entries();
    let dependent_entry = entries
        .iter()
        .find(|entry| entry.name == "dep-on-failure-and-cancel")
        .expect("dependent should be recorded");
    assert_eq!(dependent_entry.status, TaskStatus::Skipped);
    assert_eq!(
        dependent_entry.message.as_deref(),
        Some("blocked by failed dependency: failed-task")
    );
}
