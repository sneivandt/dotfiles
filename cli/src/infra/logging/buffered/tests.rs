use super::super::types::OutputExt as _;
use super::*;
use crate::infra::logging::isolated_logger;
use crate::infra::logging::{ActionCounts, TaskEntry, TaskVisibility};

/// A [`BufferedLog`] wired to a [`Logger`] that writes to a temporary run
/// log, plus the temp dir and dispatch guard that must outlive the test.
fn buffered_fixture() -> (
    BufferedLog,
    Arc<Logger>,
    tempfile::TempDir,
    crate::infra::logging::TestDispatchGuard,
) {
    let (log, tmp, guard) = isolated_logger();
    let log = Arc::new(log);
    let buf = BufferedLog::new(Arc::clone(&log));
    (buf, log, tmp, guard)
}
/// Build a buffered entry of the given kind for replay assertions.
fn entry(kind: MsgKind, msg: &str) -> LogEntry {
    LogEntry {
        action: false,
        kind,
        msg: msg.to_string(),
    }
}

fn task_entry(name: &str, status: TaskStatus, actions: ActionCounts) -> TaskEntry {
    TaskEntry::new(name, name, status, None, actions, TaskVisibility::Visible)
}
use std::fs;
use std::sync::Arc;

#[test]
fn buffered_log_record_task_forwards_to_logger() {
    let (buf, log, _tmp, _guard) = buffered_fixture();
    buf.record_task(task_entry(
        "task-a",
        TaskStatus::Ok,
        ActionCounts::default(),
    ));
    assert_eq!(log.task_entries().len(), 1);
    assert_eq!(log.task_entries()[0].name, "task-a");
}

#[test]
fn buffered_log_record_task_with_actions_forwards_counts() {
    let (buf, log, _tmp, _guard) = buffered_fixture();
    let actions = ActionCounts {
        applied: 2,
        ..ActionCounts::default()
    };

    buf.record_task(task_entry("task-a", TaskStatus::Changed, actions));

    assert_eq!(log.task_entries()[0].actions, actions);
}

#[test]
fn direct_and_buffered_messages_are_persisted_once_in_emission_order() {
    for verbose in [false, true] {
        let (mut log, _tmp, _guard) = isolated_logger();
        log.set_verbose(verbose);
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        let mut previous = 0;
        for (kind, event) in [
            (MsgKind::Stage, "stage"),
            (MsgKind::TaskStage, "stage"),
            (MsgKind::Info, "info"),
            (MsgKind::Debug, "debug"),
            (MsgKind::Trace, "debug"),
            (MsgKind::Warn, "warn"),
            (MsgKind::Error, "error"),
            (MsgKind::DryRun, "dry_run"),
            (MsgKind::Always, "info"),
            (MsgKind::Startup, "info"),
        ] {
            let outputs: [(&str, &dyn Output); 2] = [("buffered", &buf), ("direct", log.as_ref())];
            for (sink, output) in outputs {
                let message = format!("{sink} {kind:?}");
                output.emit(kind, message.clone().into());
                let contents = fs::read_to_string(log.log_path().unwrap()).unwrap();
                let position = contents.find(&format!("[{event}] {message}")).unwrap();
                assert!(
                    position > previous,
                    "{message}: must persist immediately and chronologically"
                );
                assert_eq!(
                    contents.matches(&message).count(),
                    1,
                    "{message}: written once"
                );
                previous = position;
            }
        }
        assert_eq!(buf.entries.lock().unwrap().len(), 10);
        let before_flush = fs::read_to_string(log.log_path().unwrap()).unwrap();
        buf.flush();
        buf.flush();
        assert!(buf.entries.lock().unwrap().is_empty());
        assert_eq!(
            fs::read_to_string(log.log_path().unwrap()).unwrap(),
            before_flush,
            "replaying console output must neither reorder nor duplicate persistent events"
        );
    }
}

#[test]
fn completion_order_and_action_barriers_survive_buffered_flush() {
    for verbose in [false, true] {
        let (mut log, _tmp, _guard) = isolated_logger();
        log.set_verbose(verbose);
        let log = Arc::new(log);
        let alpha = BufferedLog::new(Arc::clone(&log));
        let beta = BufferedLog::new(Arc::clone(&log));
        alpha.info("linked: z");
        alpha.info("linked: a");
        alpha.info("context");
        alpha.info("installed: z");
        alpha.info("installed: a");
        beta.info("configured: b");

        for (buf, name) in [(&beta, "beta"), (&alpha, "alpha")] {
            buf.record_task(task_entry(
                name,
                TaskStatus::Changed,
                ActionCounts::default(),
            ));
            buf.flush_and_complete(name, name, TaskStatus::Changed);
        }
        assert_eq!(
            log.task_entries()
                .iter()
                .map(|task| task.name.as_str())
                .collect::<Vec<_>>(),
            ["beta", "alpha"],
            "task results retain completion order rather than task-name order"
        );
        let details = log.lock_task_details().clone();
        assert_eq!(details[0].task_id, "beta");
        assert_eq!(details[0].lines, ["configured: b"]);
        assert_eq!(details[1].task_id, "alpha");
        assert_eq!(
            details[1].lines,
            [
                "linked: a",
                "linked: z",
                "context",
                "installed: a",
                "installed: z"
            ],
            "only consecutive action runs may be reordered"
        );
        let contents = fs::read_to_string(log.log_path().unwrap()).unwrap();
        assert!(
            contents.find("linked: z").unwrap() < contents.find("linked: a").unwrap(),
            "console sorting must not reorder chronological run-log events"
        );
    }
}

#[test]
#[allow(
    clippy::panic,
    reason = "the test intentionally poisons the mutex to verify recovery"
)]
fn buffered_log_recovers_from_a_poisoned_entry_lock() {
    let (buf, _log, _tmp, _dispatch_guard) = buffered_fixture();
    let _poison = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _poison_guard = buf.entries.lock().unwrap();
        panic!("intentional mutex poison");
    }));

    buf.info("recorded after poison");

    let entries = buf
        .entries
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(
        entries.len(),
        1,
        "poison recovery should preserve new entries"
    );
    assert_eq!(
        entries[0].msg, "recorded after poison",
        "the post-poison entry should be buffered"
    );
    drop(entries);
}

#[test]
fn flush_and_complete_clears_progress_rows() {
    let (log, _tmp, _guard) = isolated_logger();
    let log = Arc::new(log);
    log.notify_task_start("update");
    let buf = BufferedLog::new(Arc::clone(&log));
    buf.flush_and_complete("update", "update", TaskStatus::Ok);
    assert_eq!(
        log.progress_rows_count(),
        0,
        "progress_rows should be zero after all tasks complete"
    );
}

#[test]
fn buffered_presentation_golden_matrix() {
    let (_buf, log, _tmp, _guard) = buffered_fixture();
    for (kind, verbose, warning, detail) in [
        (MsgKind::Stage, false, false, false),
        (MsgKind::TaskStage, false, false, false),
        (MsgKind::Info, true, false, true),
        (MsgKind::Debug, true, false, false),
        (MsgKind::Trace, false, false, false),
        (MsgKind::Warn, true, true, false),
        (MsgKind::Error, true, true, false),
        (MsgKind::DryRun, true, false, true),
        (MsgKind::Always, true, false, true),
        (MsgKind::Startup, true, false, false),
    ] {
        let entry = entry(kind, "detail");
        assert_eq!(entry.replay_verbose(&log, None), verbose, "{kind:?}");
        assert!(
            !entry.replay_verbose(&log, Some("detail")),
            "{kind:?}: duplicate reason"
        );
        for status in [
            TaskStatus::Ok,
            TaskStatus::Changed,
            TaskStatus::Passed,
            TaskStatus::NotApplicable,
            TaskStatus::Skipped,
            TaskStatus::DryRun,
            TaskStatus::Failed,
        ] {
            assert_eq!(
                entry.detail_line(status),
                (detail || (warning && status == TaskStatus::Failed)).then_some("detail"),
                "{kind:?}, {status:?}: summary detail"
            );
            assert_eq!(
                entry.is_visible_in_non_verbose(status, None),
                warning && status != TaskStatus::Failed,
                "{kind:?}, {status:?}: non-verbose console"
            );
            assert!(
                !entry.is_visible_in_non_verbose(status, Some("detail")),
                "{kind:?}, {status:?}: duplicate reason"
            );
        }
    }
    for message in [
        "skipped: reason",
        "skipping: reason",
        "failed: reason",
        "interrupted: reason",
        "3 changed, 1 already ok",
    ] {
        assert!(
            !entry(MsgKind::Info, message).replay_verbose(&log, Some("reason")),
            "{message}"
        );
    }
}

#[test]
fn buffered_log_run_log_returns_inner_run_log() {
    let (buf, log, _tmp, _guard) = buffered_fixture();
    assert_eq!(
        buf.run_log().is_some(),
        log.run_log().is_some(),
        "BufferedLog::run_log() should match the logger's run log"
    );
}

#[test]
#[allow(clippy::significant_drop_tightening, reason = "intentional lock scope")]
fn buffered_flush_and_complete_with_remaining_task() {
    let (log, _tmp, _guard) = isolated_logger();
    let log = Arc::new(log);
    log.notify_task_start("task-a");
    log.notify_task_start("task-b");
    let buf = BufferedLog::new(Arc::clone(&log));
    buf.flush_and_complete("task-a", "task-a", TaskStatus::Ok);
    let active = log.active_tasks.lock().unwrap();
    assert!(
        active.contains(&"task-b".to_string()),
        "task-b should still be in active tasks"
    );
}

/// Regression test: tasks that produce stats output by calling
/// `ctx.log().info()` inside `run()` — as `process_resources` does via
/// central stats reporting — must have their stage header recorded
/// by `flush_and_complete()`.
///
/// Before this was caught, tasks producing `"0 changed, X already ok"`
/// output were observed without their stage headers in the persistent log.
#[test]
fn flush_and_complete_replays_stage_before_info() {
    let (buf, log, _tmp, _guard) = buffered_fixture();

    // Simulate the order execute() and central stats reporting produce entries:
    // execute() calls ctx.log().stage() first, then run() calls ctx.log().info()
    // via stats.finish() before returning Ok.
    buf.stage("install-task");
    buf.info("0 changed, 37 already ok");

    buf.flush_and_complete("install-task", "install-task", TaskStatus::Ok);

    let path = log.log_path().expect("log path");
    let contents = fs::read_to_string(path).unwrap();

    let stage_pos = contents
        .find("[stage] install-task")
        .expect("stage header must appear in log after flush_and_complete");
    let info_pos = contents
        .find("0 changed, 37 already ok")
        .expect("stats info must appear in log after flush_and_complete");

    assert!(
        stage_pos < info_pos,
        "stage header must come before stats info\nlog:\n{contents}"
    );
}

/// Regression test: the stage header must appear even when `notify_task_start`
/// has been called first (i.e., a progress row is active), as happens in the
/// parallel scheduler where `notify_task_start` precedes `execute()`.
#[test]
fn flush_and_complete_replays_stage_after_progress_clear() {
    let (log, _tmp, _guard) = isolated_logger();
    let log = Arc::new(log);

    // Simulate parallel scheduler: notify_task_start before execute().
    log.notify_task_start("parallel-task");

    let buf = BufferedLog::new(Arc::clone(&log));
    buf.stage("parallel-task");
    buf.info("0 changed, 1 already ok");

    buf.flush_and_complete("parallel-task", "parallel-task", TaskStatus::Ok);

    let path = log.log_path().expect("log path");
    let contents = fs::read_to_string(path).unwrap();

    assert!(
        contents.contains("[stage] parallel-task"),
        "stage header must appear after flush_and_complete even when progress row was active\nlog:\n{contents}"
    );
    assert!(
        contents.contains("0 changed, 1 already ok"),
        "stats info must appear\nlog:\n{contents}"
    );
}

#[test]
fn verbose_flush_keeps_not_applicable_task_output_off_console() {
    let (buf, log, _tmp, _guard) = buffered_fixture();
    buf.task_stage("windows-only-task");
    buf.debug("not applicable: requires Windows");

    buf.flush_and_complete(
        "windows-only-task",
        "windows-only-task",
        TaskStatus::NotApplicable,
    );

    assert!(!log.task_console_output_emitted());
    let path = log.log_path().expect("log path");
    let contents = fs::read_to_string(path).unwrap();
    assert!(contents.contains("windows-only-task"));
    assert!(contents.contains("not applicable: requires Windows"));
}

#[test]
fn verbose_flush_keeps_unchanged_task_output_off_console() {
    let (buf, log, _tmp, _guard) = buffered_fixture();
    buf.task_stage("current-task");
    buf.info("0 changed, 1 already ok");

    buf.flush_and_complete("current-task", "current-task", TaskStatus::Ok);

    assert!(!log.task_console_output_emitted());
    let path = log.log_path().expect("log path");
    let contents = fs::read_to_string(path).unwrap();
    assert!(contents.contains("current-task"));
    assert!(contents.contains("0 changed, 1 already ok"));
}

#[test]
fn non_verbose_dry_run_flush_keeps_detail_in_persistent_log() {
    let (mut log, _tmp, _guard) = isolated_logger();
    log.set_verbose(false);
    let log = Arc::new(log);
    let buf = BufferedLog::new(Arc::clone(&log));

    buf.dry_run("would configure beep = true");
    buf.flush_and_complete("Configure Copilot", "Configure Copilot", TaskStatus::DryRun);

    let details = log
        .task_details
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(details.len(), 1);
    assert_eq!(details[0].task_id, "Configure Copilot");
    assert_eq!(details[0].lines, ["would configure beep = true"]);

    let path = log.log_path().expect("log path");
    let contents = fs::read_to_string(path).unwrap();
    assert!(
        contents.contains("would configure beep = true"),
        "dry-run details should still be written to the persistent log"
    );
    assert!(
        contents.contains("[dry_run] would configure beep = true"),
        "dry-run details are recorded under the dry_run event kind\nlog:\n{contents}"
    );
}

#[test]
fn typed_actions_persist_once_before_flush_with_arbitrary_verbs() {
    use crate::infra::logging::records::{Record, StoredRecord};
    let (buf, log, _tmp, _guard) = buffered_fixture();
    buf.action("refresh", "z-item", true, "refresh z-item");
    buf.warn("barrier");
    buf.action("refresh", "b-item", true, "refresh b-item");
    buf.action("refresh", "a-item", true, "refresh a-item");
    let before = fs::read_to_string(log.log_path().unwrap()).unwrap();
    let subjects: Vec<_> = before
        .lines()
        .filter_map(StoredRecord::from_line)
        .filter_map(|entry| {
            if let Record::Action {
                subject, planned, ..
            } = entry.record
            {
                assert!(planned);
                Some(subject)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(subjects, ["z-item", "b-item", "a-item"]);
    buf.flush();
    assert_eq!(fs::read_to_string(log.log_path().unwrap()).unwrap(), before);
}
