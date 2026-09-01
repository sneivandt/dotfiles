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
fn buffered_log_preserves_entry_order() {
    let (buf, log, _tmp, _guard) = buffered_fixture();
    buf.stage("stage-1");
    buf.info("info-1");
    buf.debug("debug-1");
    buf.warn("warn-1");
    buf.flush();
    let path = log.log_path().expect("log path");
    let contents = fs::read_to_string(path).unwrap();
    let stage_pos = contents.find("stage-1").expect("stage-1 in log");
    let info_pos = contents.find("info-1").expect("info-1 in log");
    let debug_pos = contents.find("debug-1").expect("debug-1 in log");
    let warn_pos = contents.find("warn-1").expect("warn-1 in log");
    assert!(stage_pos < info_pos, "stage before info");
    assert!(info_pos < debug_pos, "info before debug");
    assert!(debug_pos < warn_pos, "debug before warn");
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
fn buffered_log_writes_to_run_log_immediately() {
    let (buf, log, _tmp, _guard) = buffered_fixture();
    let marker = format!("buf-runlog-{}", std::process::id());
    buf.info(&marker);
    let path = log.log_path().expect("log path");
    let contents = fs::read_to_string(path).unwrap();
    assert!(
        contents.contains(&marker),
        "BufferedLog should write to the run log immediately, not after flush"
    );
}

#[test]
fn log_entry_replay_all_variants() {
    let (buf, log, _tmp, _guard) = buffered_fixture();
    let pid = std::process::id();
    buf.stage(format!("replay-stage-{pid}"));
    buf.info(format!("replay-info-{pid}"));
    buf.debug(format!("replay-debug-{pid}"));
    buf.warn(format!("replay-warn-{pid}"));
    buf.error(format!("replay-error-{pid}"));
    buf.dry_run(format!("replay-dryrun-{pid}"));
    buf.flush();
    let path = log.log_path().expect("log path");
    let contents = fs::read_to_string(path).unwrap();
    assert!(contents.contains(&format!("replay-stage-{pid}")));
    assert!(contents.contains(&format!("replay-info-{pid}")));
    assert!(contents.contains(&format!("replay-debug-{pid}")));
    assert!(contents.contains(&format!("replay-warn-{pid}")));
    assert!(contents.contains(&format!("replay-error-{pid}")));
    assert!(contents.contains(&format!("replay-dryrun-{pid}")));
}

#[test]
fn buffered_log_all_variants_buffered() {
    let (buf, log, _tmp, _guard) = buffered_fixture();
    let pid = std::process::id();
    buf.info(format!("all-info-{pid}"));
    buf.warn(format!("all-warn-{pid}"));
    buf.error(format!("all-error-{pid}"));
    buf.dry_run(format!("all-dryrun-{pid}"));
    buf.debug(format!("all-debug-{pid}"));
    buf.flush();
    let path = log.log_path().expect("log path");
    let contents = fs::read_to_string(path).unwrap();
    assert!(contents.contains(&format!("all-info-{pid}")));
    assert!(contents.contains(&format!("all-warn-{pid}")));
    assert!(contents.contains(&format!("all-error-{pid}")));
    assert!(contents.contains(&format!("all-dryrun-{pid}")));
    assert!(contents.contains(&format!("all-debug-{pid}")));
}

#[derive(Clone, Debug)]
struct TargetCaptureLayer {
    targets: Arc<Mutex<Vec<String>>>,
}

impl<S> tracing_subscriber::Layer<S> for TargetCaptureLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        self.targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(event.metadata().target().to_string());
    }
}

#[test]
fn non_verbose_replay_only_shows_warnings_and_errors() {
    for entry in [
        entry(MsgKind::Stage, "stage"),
        entry(MsgKind::TaskStage, "task"),
        entry(MsgKind::Info, "info"),
        entry(MsgKind::DryRun, "dry-run"),
        entry(MsgKind::Always, "always"),
    ] {
        assert!(
            !entry.is_visible_in_non_verbose(TaskStatus::Ok, None),
            "{entry:?} should be deferred to the summary in non-verbose replay"
        );
    }

    for entry in [entry(MsgKind::Warn, "warn"), entry(MsgKind::Error, "error")] {
        assert!(
            entry.is_visible_in_non_verbose(TaskStatus::Ok, None),
            "{entry:?} must reach the console in non-verbose replay"
        );
    }
}

#[test]
fn non_verbose_failed_replay_keeps_errors_off_the_console() {
    for entry in [entry(MsgKind::Warn, "warn"), entry(MsgKind::Error, "error")] {
        assert!(
            !entry.is_visible_in_non_verbose(TaskStatus::Failed, None),
            "failed-task output surfaces through the summary, not a separate console line"
        );
    }
}

#[test]
fn verbose_replay_targets_console_ui() {
    use tracing_subscriber::layer::SubscriberExt as _;

    let targets = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(TargetCaptureLayer {
        targets: Arc::clone(&targets),
    });
    let dispatch = tracing::Dispatch::new(subscriber);
    let _guard = crate::infra::logging::test_dispatch_guard(&dispatch);

    for entry in [
        entry(MsgKind::Stage, "stage"),
        entry(MsgKind::TaskStage, "task"),
        entry(MsgKind::Info, "info"),
        entry(MsgKind::Warn, "warn"),
    ] {
        entry.replay_verbose(None);
    }

    let targets = targets
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(
        targets,
        vec!["dotfiles::ui::info", "dotfiles::ui::warn"],
        "verbose replay renders console targets and suppresses stage headers"
    );
}

#[test]
fn failed_task_errors_become_task_details() {
    let warning = entry(MsgKind::Warn, "failed: package install");
    let error = entry(MsgKind::Error, "packages: command failed");

    assert_eq!(
        warning.detail_line(TaskStatus::Failed),
        Some("failed: package install")
    );
    assert_eq!(
        error.detail_line(TaskStatus::Failed),
        Some("packages: command failed")
    );
    assert_eq!(warning.detail_line(TaskStatus::Ok), None);
    assert_eq!(error.detail_line(TaskStatus::Ok), None);
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
