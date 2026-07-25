#![allow(
    clippy::panic,
    reason = "exhaustive match arms assert the unexpected variant by panicking"
)]

use crate::engine::stats::ItemOutcome;
use crate::engine::{TaskResult, TaskStats};
// -----------------------------------------------------------------------
// TaskStats
// -----------------------------------------------------------------------

#[test]
fn stats_record_increments_each_outcome() {
    let mut stats = TaskStats::new();

    stats.record(ItemOutcome::Changed);
    stats.record(ItemOutcome::AlreadyOk);
    stats.record(ItemOutcome::Skipped);
    stats.record(ItemOutcome::Failed);

    assert_eq!(stats.changed, 1);
    assert_eq!(stats.already_ok, 1);
    assert_eq!(stats.skipped, 1);
    assert_eq!(stats.failed, 1);
}

#[test]
fn stats_record_saturates_each_outcome() {
    let mut stats = TaskStats {
        changed: u32::MAX,
        already_ok: u32::MAX,
        skipped: u32::MAX,
        failed: u32::MAX,
        message: None,
    };

    stats.record(ItemOutcome::Changed);
    stats.record(ItemOutcome::AlreadyOk);
    stats.record(ItemOutcome::Skipped);
    stats.record(ItemOutcome::Failed);

    assert_eq!(stats.changed, u32::MAX);
    assert_eq!(stats.already_ok, u32::MAX);
    assert_eq!(stats.skipped, u32::MAX);
    assert_eq!(stats.failed, u32::MAX);
}

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

// -------------------------------------------------------------------
// TaskStats construction
// -------------------------------------------------------------------

#[test]
fn new_stats_are_all_zero() {
    let stats = TaskStats::new();
    assert_eq!(stats.changed, 0);
    assert_eq!(stats.already_ok, 0);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.failed, 0);
}

#[test]
fn default_stats_are_all_zero() {
    let stats = TaskStats::default();
    assert_eq!(stats.changed, 0);
    assert_eq!(stats.already_ok, 0);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.failed, 0);
}

// -------------------------------------------------------------------
// TaskStats::summary
// -------------------------------------------------------------------

#[test]
fn summary_all_zeros() {
    let stats = TaskStats::new();
    assert_eq!(stats.summary(false), "0 changed, 0 already ok");
}

#[test]
fn summary_all_zeros_dry_run() {
    let stats = TaskStats::new();
    assert_eq!(stats.summary(true), "0 would change, 0 already ok");
}

#[test]
fn summary_changed_only() {
    let stats = TaskStats {
        changed: 5,
        already_ok: 0,
        skipped: 0,
        failed: 0,
        message: None,
    };
    assert_eq!(stats.summary(false), "5 changed, 0 already ok");
}

#[test]
fn summary_already_ok_only() {
    let stats = TaskStats {
        changed: 0,
        already_ok: 10,
        skipped: 0,
        failed: 0,
        message: None,
    };
    assert_eq!(stats.summary(false), "0 changed, 10 already ok");
}

#[test]
fn summary_with_skipped() {
    let stats = TaskStats {
        changed: 1,
        already_ok: 2,
        skipped: 3,
        failed: 0,
        message: None,
    };
    assert_eq!(stats.summary(false), "1 changed, 2 already ok, 3 skipped");
}

#[test]
fn summary_with_skipped_dry_run() {
    let stats = TaskStats {
        changed: 1,
        already_ok: 2,
        skipped: 3,
        failed: 1,
        message: None,
    };
    assert_eq!(
        stats.summary(true),
        "1 would change, 2 already ok, 3 skipped, 1 failed"
    );
}

#[test]
fn summary_hides_skipped_when_zero() {
    let stats = TaskStats {
        changed: 3,
        already_ok: 7,
        skipped: 0,
        failed: 0,
        message: None,
    };
    let s = stats.summary(false);
    assert!(!s.contains("skipped"), "should not mention skipped: {s}");
}

// -------------------------------------------------------------------
// TaskStats::finish
// -------------------------------------------------------------------

#[test]
fn finish_returns_batch_when_changes_were_recorded() {
    let stats = TaskStats {
        changed: 1,
        already_ok: 0,
        skipped: 0,
        failed: 0,
        message: None,
    };
    assert!(matches!(
        stats.finish(),
        TaskResult::Batch(stats) if stats.changed == 1
    ));
}

#[test]
fn finish_returns_batch_when_no_changes_were_recorded() {
    let stats = TaskStats {
        changed: 0,
        already_ok: 1,
        skipped: 0,
        failed: 0,
        message: None,
    };
    assert!(matches!(
        stats.finish(),
        TaskResult::Batch(stats) if stats.already_ok == 1
    ));
}

#[test]
fn finish_returns_batch_when_only_resource_skips_were_recorded() {
    let stats = TaskStats {
        changed: 0,
        already_ok: 0,
        skipped: 1,
        failed: 0,
        message: None,
    };
    assert!(matches!(
        stats.finish(),
        TaskResult::Batch(stats) if stats.skipped == 1
    ));
}

#[test]
fn finish_returns_batch_when_dry_run() {
    let stats = TaskStats {
        changed: 1,
        already_ok: 0,
        skipped: 0,
        failed: 0,
        message: None,
    };
    assert!(matches!(
        stats.finish(),
        TaskResult::Batch(stats) if stats.changed == 1
    ));
}

#[test]
fn finish_returns_batch_when_dry_run_has_no_changes() {
    let stats = TaskStats {
        changed: 0,
        already_ok: 1,
        skipped: 0,
        failed: 0,
        message: None,
    };
    assert!(matches!(
        stats.finish(),
        TaskResult::Batch(stats) if stats.already_ok == 1
    ));
}

#[test]
fn finish_returns_batch_when_non_fatal_failures_were_recorded() {
    let stats = TaskStats {
        changed: 0,
        already_ok: 1,
        skipped: 0,
        failed: 1,
        message: None,
    };

    assert!(matches!(
        stats.finish(),
        TaskResult::Batch(stats) if stats.failed == 1
    ));
}

// -------------------------------------------------------------------
// AddAssign
// -------------------------------------------------------------------

#[test]
fn add_assign_accumulates_all_fields() {
    let mut a = TaskStats {
        changed: 1,
        already_ok: 2,
        skipped: 3,
        failed: 4,
        message: None,
    };
    let b = TaskStats {
        changed: 10,
        already_ok: 20,
        skipped: 30,
        failed: 40,
        message: None,
    };
    a += b;
    assert_eq!(a.changed, 11);
    assert_eq!(a.already_ok, 22);
    assert_eq!(a.skipped, 33);
    assert_eq!(a.failed, 44);
}

#[test]
fn add_assign_with_zero_is_identity() {
    let mut a = TaskStats {
        changed: 5,
        already_ok: 3,
        skipped: 1,
        failed: 2,
        message: None,
    };
    a += TaskStats::new();
    assert_eq!(a.changed, 5);
    assert_eq!(a.already_ok, 3);
    assert_eq!(a.skipped, 1);
    assert_eq!(a.failed, 2);
}

// -------------------------------------------------------------------
// TaskResult variants
// -------------------------------------------------------------------

#[test]
fn task_result_ok_matches() {
    assert!(matches!(TaskResult::Ok, TaskResult::Ok));
}

#[test]
fn task_result_not_applicable_carries_reason() {
    let r = TaskResult::NotApplicable("no config".into());
    match r {
        TaskResult::NotApplicable(reason) => assert_eq!(reason, "no config"),
        other @ (TaskResult::Ok
        | TaskResult::DryRun
        | TaskResult::CheckPassed
        | TaskResult::Skipped(_)
        | TaskResult::Failed(_)
        | TaskResult::Batch(_)) => panic!("expected NotApplicable, got {other:?}"),
    }
}

#[test]
fn task_result_skipped_carries_reason() {
    let r = TaskResult::Skipped("wrong platform".into());
    match r {
        TaskResult::Skipped(reason) => assert_eq!(reason, "wrong platform"),
        other @ (TaskResult::Ok
        | TaskResult::DryRun
        | TaskResult::CheckPassed
        | TaskResult::NotApplicable(_)
        | TaskResult::Failed(_)
        | TaskResult::Batch(_)) => panic!("expected Skipped, got {other:?}"),
    }
}

#[test]
fn task_result_failed_carries_reason() {
    let r = TaskResult::Failed("git pull failed".into());
    match r {
        TaskResult::Failed(reason) => assert_eq!(reason, "git pull failed"),
        other @ (TaskResult::Ok
        | TaskResult::DryRun
        | TaskResult::CheckPassed
        | TaskResult::NotApplicable(_)
        | TaskResult::Skipped(_)
        | TaskResult::Batch(_)) => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn task_result_debug_format() {
    let r = TaskResult::Ok;
    assert_eq!(format!("{r:?}"), "Ok");
}

#[test]
fn task_result_clone() {
    let r = TaskResult::Skipped("reason".into());
    #[allow(clippy::redundant_clone, reason = "clone keeps test intent explicit")]
    let r2 = r.clone();
    assert!(matches!(r2, TaskResult::Skipped(_)));
}
