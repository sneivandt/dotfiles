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
    };
    assert_eq!(stats.summary(false), "3 changed, 0 already ok");
}

#[test]
fn stats_summary_dry_run() {
    let stats = TaskStats {
        changed: 2,
        already_ok: 5,
        skipped: 0,
    };
    assert_eq!(stats.summary(true), "2 would change, 5 already ok");
}

#[test]
fn stats_summary_with_skipped() {
    let stats = TaskStats {
        changed: 1,
        already_ok: 2,
        skipped: 3,
    };
    assert_eq!(stats.summary(false), "1 changed, 2 already ok, 3 skipped");
}

#[test]
fn stats_finish_returns_dry_run_result() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = dry_run_context(config);
    let stats = TaskStats::new();
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

// -----------------------------------------------------------------------
// TaskStats AddAssign
// -----------------------------------------------------------------------

#[test]
fn stats_add_assign_accumulates() {
    let mut a = TaskStats {
        changed: 1,
        already_ok: 2,
        skipped: 3,
    };
    let b = TaskStats {
        changed: 10,
        already_ok: 20,
        skipped: 30,
    };
    a += b;
    assert_eq!(a.changed, 11);
    assert_eq!(a.already_ok, 22);
    assert_eq!(a.skipped, 33);
}
