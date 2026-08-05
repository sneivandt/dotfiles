//! Stage-header replay regressions for buffered task output.

use super::*;

/// Regression test: stage header must be present in the log when a task
/// returns batch statistics for central reporting.
///
/// Before the regression was detected, tasks producing `"0 changed, X
/// already ok"` output via `process_resources` were missing their `==>`
/// stage headers in the persistent log.
#[test]
fn stage_header_present_when_info_logged_in_run() {
    let (log, _tmp, _guard) = logging::isolated_logger();
    let log = Arc::new(log);

    let ctx = ContextBuilder::new(empty_config(PathBuf::from("/tmp"))).build();
    let buf = Arc::new(BufferedLog::new(Arc::clone(&log)));
    let task_ctx = ctx.with_log(buffered_log_arc(&buf));

    // Exactly mirrors what run_tasks_parallel does per task thread.
    log.notify_task_start("stats-task");
    let status = execute(&StatsTask, &task_ctx);
    buf.flush_and_complete("stats-task", status);

    let path = log.log_path().expect("log path");
    let contents = std::fs::read_to_string(path).unwrap();

    let stage_pos = contents
        .find("[stage] stats-task")
        .expect("stage header must appear in log for task that calls ctx.log().info in run()");
    let info_pos = contents
        .find("0 changed, 37 already ok")
        .expect("stats info must appear in log");

    assert!(
        stage_pos < info_pos,
        "stage header must come before stats info; log:\n{contents}"
    );
}

/// Regression test: stage header must be present for multiple parallel tasks
/// that all produce stats output.  Simulates the scenario where 6 dependent
/// tasks start after `ReloadConfig` and all complete with `"0 changed, X
/// already ok"` output, none of which should be missing their stage header.
#[test]
fn stage_headers_present_for_multiple_concurrent_stats_tasks() {
    let (log, _tmp, _guard) = logging::isolated_logger();
    let log = Arc::new(log);

    let tasks_to_run: &[(&str, u32)] = &[
        ("install-symlinks", 37),
        ("apply-permissions", 3),
        ("configure-systemd", 2),
        ("install-hooks", 1),
    ];

    // Run each task through the same per-thread flow used by run_tasks_parallel.
    for (name, count) in tasks_to_run {
        let ctx = ContextBuilder::new(empty_config(PathBuf::from("/tmp"))).build();
        let buf = Arc::new(BufferedLog::new(Arc::clone(&log)));
        let task_named = NamedStatsTask {
            name,
            count: *count,
        };
        let task_ctx = ctx.with_log(buffered_log_arc(&buf));

        log.notify_task_start(name);
        let status = execute(&task_named, &task_ctx);
        buf.flush_and_complete(name, status);
    }

    let path = log.log_path().expect("log path");
    let contents = std::fs::read_to_string(path).unwrap();

    for (name, count) in tasks_to_run {
        assert!(
            contents.contains(&format!("[stage] {name}")),
            "stage header must appear for task '{name}'; log:\n{contents}"
        );
        assert!(
            contents.contains(&format!("0 changed, {count} already ok")),
            "stats info must appear for task '{name}'; log:\n{contents}"
        );
    }
}

/// Regression test: task status must not be lost when a task calls
/// `ctx.debug_fmt()` during `run()`.
///
/// The regression was introduced by `debug_fmt()` using
/// `tracing::enabled!(Level::DEBUG)` as a guard.  That macro creates a
/// HINT-kind callsite that goes through `Subscriber::enabled()`,
/// setting per-layer `FilterState` bits on the calling thread without
/// dispatching an event to clean them up.  With a two-layer subscriber
/// (INFO-level console + DEBUG-level file), the stale bits caused the
/// `Filtered` console layer to silently drop the subsequent
/// `tracing::info!(target: "dotfiles::task_result", …)` emission.
///
/// This test uses a lightweight custom `Layer` (rather than
/// `tracing_subscriber::fmt::Layer`) to record which event targets pass
/// through a `LevelFilter::INFO` filter.  Using a custom layer avoids
/// platform-specific differences in `fmt::Layer` formatting/writing
/// while still exercising the `Filtered` machinery that the original bug
/// corrupted.
#[test]
fn task_status_not_lost_after_debug_fmt_call() {
    use std::sync::{Arc, Mutex};
    use tracing::Subscriber;
    use tracing_subscriber::{
        Layer as TracingLayer, filter::LevelFilter, layer::SubscriberExt as _,
    };

    /// Minimal layer that records the target of every event it receives.
    struct TargetCapture(Arc<Mutex<Vec<String>>>);

    impl<S: Subscriber> TracingLayer<S> for TargetCapture {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _cx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(event.metadata().target().to_string());
        }
    }

    // Task that calls ctx.debug_fmt() — simulating what apply.rs does
    // for per-resource "ok: <desc>" messages.
    struct DebugFmtTask;
    impl Task for DebugFmtTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("debug-fmt-task")
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, ctx: &Context) -> Result<TaskResult> {
            // This is the call site that the regression broke:
            // debug_fmt previously used tracing::enabled!(DEBUG) which
            // left stale FilterState bits that silently dropped the
            // subsequent stage INFO event replayed by flush_and_complete.
            ctx.debug_fmt(|| "ok: some/resource".to_string());
            ctx.log().info("1 changed, 0 already ok");
            Ok(TaskStats {
                changed: 1,
                ..TaskStats::default()
            }
            .finish())
        }
    }

    // Two-layer subscriber: INFO-filtered capture (simulates console) +
    // DEBUG-filtered file (simulates the diagnostic log). This is the
    // topology where the `FilterState` corruption originally manifested.
    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let (log, _tmp, _guard) = logging::isolated_logger();
    let log = Arc::new(log);
    let info_layer = TargetCapture(Arc::clone(&captured)).with_filter(LevelFilter::INFO);
    let subscriber = tracing_subscriber::registry().with(info_layer);
    let _inner_guard = tracing::dispatcher::set_default(&tracing::Dispatch::new(subscriber));

    let ctx = ContextBuilder::new(empty_config(PathBuf::from("/tmp"))).build();
    let buf = Arc::new(BufferedLog::new(Arc::clone(&log)));
    let task_ctx = ctx.with_log(buffered_log_arc(&buf));

    log.notify_task_start("debug-fmt-task");
    let status = execute(&DebugFmtTask, &task_ctx);
    buf.flush_and_complete("debug-fmt-task", status);

    let targets = captured
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert!(
        targets.iter().any(|t| t == "dotfiles::ui::task_result"),
        "task status must reach the INFO-filtered layer after debug_fmt() was called;\nreceived targets:\n{targets:?}"
    );
}
