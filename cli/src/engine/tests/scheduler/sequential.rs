//! Sequential runner behaviour, including panics and recorded details.

use super::*;

#[test]
fn sequential_runner_skips_dependents_when_dependency_fails() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran = Arc::new(AtomicBool::new(false));
    let failed_task = FailedTask;
    let dep_task = DepOnFailedTask {
        ran: Arc::clone(&ran),
    };
    let tasks: Vec<&dyn Task> = vec![&failed_task, &dep_task];
    let graph = ResolvedTaskGraph::resolve(&tasks).unwrap();

    run_tasks_sequential(&tasks, &graph, &ctx, &log);

    assert!(
        !ran.load(Ordering::SeqCst),
        "dependent task should not have run"
    );
    let entries = log.task_entries();
    assert!(
        entries
            .iter()
            .any(|e| e.name == "failed-task" && e.status == TaskStatus::Failed),
        "failed task should be recorded as Failed"
    );
    assert!(
        entries
            .iter()
            .any(|e| e.name == "dep-on-failed" && e.status == TaskStatus::Skipped),
        "dependent task should be recorded as Skipped"
    );
}

#[test]
fn sequential_runner_records_panics_as_failures() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran = Arc::new(AtomicBool::new(false));
    let panic_task = PanicTask;
    let dep_task = DepOnPanicTask {
        ran: Arc::clone(&ran),
    };
    let tasks: Vec<&dyn Task> = vec![&panic_task, &dep_task];
    let graph = ResolvedTaskGraph::resolve(&tasks).unwrap();

    run_tasks_sequential(&tasks, &graph, &ctx, &log);

    assert!(
        !ran.load(Ordering::SeqCst),
        "dependent task should not have run"
    );
    let entries = log.task_entries();
    assert!(
        entries
            .iter()
            .any(|e| e.name == "panic-task" && e.status == TaskStatus::Failed),
        "panicked task should be recorded as Failed"
    );
    assert!(
        entries
            .iter()
            .any(|e| e.name == "dep-on-panic" && e.status == TaskStatus::Skipped),
        "dependent task should be recorded as Skipped"
    );
}

#[test]
fn dependency_block_reason_is_owned_by_recorded_task_result() {
    #[derive(Default)]
    struct RecordingLog {
        info_lines: std::sync::Mutex<Vec<String>>,
        debug_lines: std::sync::Mutex<Vec<String>>,
        records: std::sync::Mutex<Vec<TaskStatus>>,
    }

    impl Output for RecordingLog {
        fn emit(&self, kind: MsgKind, msg: std::borrow::Cow<'_, str>) {
            let sink = match kind {
                MsgKind::Info => &self.info_lines,
                MsgKind::Debug => &self.debug_lines,
                MsgKind::Stage
                | MsgKind::TaskStage
                | MsgKind::Warn
                | MsgKind::Error
                | MsgKind::DryRun
                | MsgKind::Always => return,
            };
            sink.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(msg.into_owned());
        }
    }

    impl TaskRecorder for RecordingLog {
        fn record_task(&self, _name: &str, status: TaskStatus, _message: Option<&str>) {
            self.records
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(status);
        }
    }

    let log = RecordingLog::default();
    let ran = Arc::new(AtomicBool::new(false));
    let task = DepOnFailedTask { ran };

    record_scheduler_skip(&task, &log, "dependency failed");

    let info_lines = log
        .info_lines
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert!(
        info_lines.is_empty(),
        "dependency skip reason should not be emitted before its task status: {info_lines:?}"
    );
    let debug_lines = log
        .debug_lines
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(
        debug_lines,
        ["dependency failed"],
        "dependency skip reason should remain in the persistent debug log"
    );
    let records = log
        .records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert!(
        records.contains(&TaskStatus::Skipped),
        "dependency block should still record a skipped task"
    );
}

#[test]
fn sequential_runner_records_details_for_final_summary() {
    let (mut log, _tmp, _guard) = logging::isolated_logger();
    log.set_verbose(false);
    let log = Arc::new(log);
    let log_output: Arc<dyn Log> = Arc::<Logger>::clone(&log);
    let ctx = ContextBuilder::new(empty_config(PathBuf::from("/tmp")))
        .build()
        .with_log(log_output);
    let task = SequentialChangedDetailTask;
    let tasks: Vec<&dyn Task> = vec![&task];
    let graph = ResolvedTaskGraph::resolve(&tasks).unwrap();

    run_tasks_sequential(&tasks, &graph, &ctx, &log);
    log.print_summary();

    let path = log.log_path().expect("log path");
    let contents = std::fs::read_to_string(path).unwrap();
    let detail_occurrences = contents.matches("installed: demo-package").count();
    assert_eq!(
        detail_occurrences, 1,
        "detail should be written during task flush but not repeated in the final file summary; log:\n{contents}"
    );
}
