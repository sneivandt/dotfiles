use crate::engine::{TaskResult, TaskStats};
// -----------------------------------------------------------------------
// TaskStats
// -----------------------------------------------------------------------

#[test]
fn stats_summary_changed_only() {
    let stats = TaskStats {
        changed: 3,
        already_ok: 0,
        skipped: 0,
        failed: 0,
        ..TaskStats::default()
    };
    assert_eq!(stats.summary(false), "3 changed, 0 already ok");
}

#[test]
fn stats_summary_dry_run() {
    let stats = TaskStats {
        changed: 2,
        already_ok: 5,
        skipped: 0,
        failed: 0,
        ..TaskStats::default()
    };
    assert_eq!(stats.summary(true), "2 would change, 5 already ok");
}

#[test]
fn stats_summary_with_skipped() {
    let stats = TaskStats {
        changed: 1,
        already_ok: 2,
        skipped: 3,
        failed: 0,
        ..TaskStats::default()
    };
    assert_eq!(stats.summary(false), "1 changed, 2 already ok, 3 skipped");
}

#[test]
fn stats_summary_with_failed() {
    let stats = TaskStats {
        changed: 1,
        already_ok: 2,
        skipped: 0,
        failed: 3,
        ..TaskStats::default()
    };
    assert_eq!(stats.summary(false), "1 changed, 2 already ok, 3 failed");
}

#[test]
fn stats_finish_returns_batch_when_dry_run_has_no_changes() {
    let stats = TaskStats::new();
    let result = stats.finish();
    assert!(matches!(result, TaskResult::Batch(batch) if batch.changed == 0));
}

#[test]
fn stats_finish_returns_batch_when_dry_run_would_change() {
    let mut stats = TaskStats::new();
    stats.changed = 1;
    let result = stats.finish();
    assert!(matches!(result, TaskResult::Batch(batch) if batch.changed == 1));
}

#[test]
fn stats_finish_returns_empty_batch() {
    let stats = TaskStats::new();
    let result = stats.finish();
    assert!(matches!(
        result,
        TaskResult::Batch(batch)
            if batch.changed == 0 && batch.already_ok == 0 && batch.failed == 0
    ));
}

#[test]
fn stats_finish_returns_changed_batch() {
    let mut stats = TaskStats::new();
    stats.changed = 1;
    let result = stats.finish();
    assert!(matches!(result, TaskResult::Batch(batch) if batch.changed == 1));
}

#[test]
fn stats_finish_returns_failed_batch() {
    let mut stats = TaskStats::new();
    stats.failed = 1;
    let result = stats.finish();
    assert!(matches!(result, TaskResult::Batch(batch) if batch.failed == 1));
}

// -----------------------------------------------------------------------
// TaskStats AddAssign
// -----------------------------------------------------------------------

#[test]
fn stats_add_assign_accumulates() {
    let mut a = TaskStats {
        changed: 1,
        already_ok: 2,
        skipped: 3,
        failed: 4,
        ..TaskStats::default()
    };
    let b = TaskStats {
        changed: 10,
        already_ok: 20,
        skipped: 30,
        failed: 40,
        ..TaskStats::default()
    };
    a += b;
    assert_eq!(a.changed, 11);
    assert_eq!(a.already_ok, 22);
    assert_eq!(a.skipped, 33);
    assert_eq!(a.failed, 44);
}
