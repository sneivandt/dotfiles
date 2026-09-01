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
    buf.flush_and_complete(&StatsTask.task_id().record_key(), "stats-task", status);

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
/// tasks start after a restart boundary and all complete with `"0 changed, X
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
        buf.flush_and_complete(&task_named.task_id().record_key(), name, status);
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
