use crate::engine::stats::ItemOutcome;
use crate::engine::{TaskResult, TaskStats};

fn counts(stats: &TaskStats) -> (u32, u32, u32, u32) {
    (
        stats.changed_count(),
        stats.already_ok_count(),
        stats.skipped_count(),
        stats.failed_count(),
    )
}

#[test]
fn record_increments_and_saturates_each_outcome() {
    let cases = [
        (ItemOutcome::Changed, (1, 0, 0, 0)),
        (ItemOutcome::AlreadyOk, (0, 1, 0, 0)),
        (ItemOutcome::Skipped, (0, 0, 1, 0)),
        (ItemOutcome::Failed, (0, 0, 0, 1)),
    ];

    for (outcome, expected) in cases {
        let mut stats = TaskStats::new();
        stats.record(outcome);
        assert_eq!(counts(&stats), expected, "{outcome:?}");
    }

    let mut saturated = TaskStats::from_counts(u32::MAX, u32::MAX, u32::MAX, u32::MAX);
    for (outcome, _) in cases {
        saturated.record(outcome);
    }
    assert_eq!(counts(&saturated), (u32::MAX, u32::MAX, u32::MAX, u32::MAX));
}

#[test]
fn summary_formats_every_counter_combination() {
    let cases = [
        ((0, 0, 0, 0), false, "0 changed, 0 already ok"),
        ((0, 0, 0, 0), true, "0 would change, 0 already ok"),
        ((3, 0, 0, 0), false, "3 changed, 0 already ok"),
        ((2, 5, 0, 0), true, "2 would change, 5 already ok"),
        ((0, 10, 0, 0), false, "0 changed, 10 already ok"),
        ((1, 2, 3, 0), false, "1 changed, 2 already ok, 3 skipped"),
        ((1, 2, 0, 3), false, "1 changed, 2 already ok, 3 failed"),
        (
            (1, 2, 3, 1),
            true,
            "1 would change, 2 already ok, 3 skipped, 1 failed",
        ),
    ];

    for ((changed, already_ok, skipped, failed), dry_run, expected) in cases {
        let stats = TaskStats::from_counts(changed, already_ok, skipped, failed);
        assert_eq!(stats.summary(dry_run), expected);
    }
}

#[test]
fn finish_preserves_all_batch_counts() {
    let expected = (1, 2, 3, 4);
    let TaskResult::Batch(stats) =
        TaskStats::from_counts(expected.0, expected.1, expected.2, expected.3).finish()
    else {
        panic!("finish must always return a batch result");
    };
    assert_eq!(counts(&stats), expected);
}

#[test]
fn add_assign_accumulates_and_zero_is_identity() {
    let mut stats = TaskStats::from_counts(1, 2, 3, 4);
    stats += TaskStats::from_counts(10, 20, 30, 40);
    assert_eq!(counts(&stats), (11, 22, 33, 44));

    stats += TaskStats::new();
    assert_eq!(counts(&stats), (11, 22, 33, 44));
}
