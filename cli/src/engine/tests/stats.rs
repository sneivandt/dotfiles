use crate::engine::{TaskResult, TaskStats};
use crate::tasks::test_helpers::empty_config;
use std::path::PathBuf;

use super::{dry_run_context, test_context};

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
    };
    assert_eq!(stats.summary(false), "1 changed, 2 already ok, 3 failed");
}

#[test]
fn stats_finish_returns_ok_when_dry_run_has_no_changes() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = dry_run_context(config);
    let stats = TaskStats::new();
    let result = stats.finish(&ctx);
    assert!(matches!(result, TaskResult::Ok));
}

#[test]
fn stats_finish_returns_dry_run_when_dry_run_would_change() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = dry_run_context(config);
    let mut stats = TaskStats::new();
    stats.changed = 1;
    let result = stats.finish(&ctx);
    assert!(matches!(result, TaskResult::DryRun));
}

#[test]
fn stats_finish_returns_ok_result() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let stats = TaskStats::new();
    let result = stats.finish(&ctx);
    assert!(matches!(result, TaskResult::Ok));
}

#[test]
fn stats_finish_returns_ok_with_message_when_changed() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let mut stats = TaskStats::new();
    stats.changed = 1;
    let result = stats.finish(&ctx);
    assert!(matches!(result, TaskResult::OkWithMessage(_)));
}

#[test]
fn stats_finish_returns_failed_result() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let mut stats = TaskStats::new();
    stats.failed = 1;
    let result = stats.finish(&ctx);
    assert!(matches!(result, TaskResult::Failed(_)));
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
    };
    let b = TaskStats {
        changed: 10,
        already_ok: 20,
        skipped: 30,
        failed: 40,
    };
    a += b;
    assert_eq!(a.changed, 11);
    assert_eq!(a.already_ok, 22);
    assert_eq!(a.skipped, 33);
    assert_eq!(a.failed, 44);
}
