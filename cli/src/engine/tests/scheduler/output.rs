//! Scheduler-owned stage ordering and result recording (not console styling).

use super::*;
use crate::infra::logging::{MsgKind, TaskRecorder, TaskResultDisplay, TaskVisibility};

fn with_recording_metadata(mut task: TestTask) -> TestTask {
    task.metadata = Some(
        TaskMeta::new("custom display name")
            .with_selector("custom-selector")
            .with_visibility(TaskVisibility::Internal)
            .with_result_display(TaskResultDisplay::RestartNotice),
    );
    task
}

fn assert_recording_metadata(entry: &TaskEntry, task: &TestTask) {
    assert_eq!(entry.task_id, task.log_key());
    assert_eq!(entry.name, "custom display name");
    assert_eq!(entry.selector.as_deref(), Some("custom-selector"));
    assert_eq!(entry.visibility, TaskVisibility::Internal);
    assert_eq!(entry.result_display, TaskResultDisplay::RestartNotice);
}

#[test]
fn normal_recording_preserves_metadata_and_summary_classification() {
    for (case, stats, status, is_summary) in [
        ("changed", TaskStats::changed(), TaskStatus::Changed, true),
        (
            "custom",
            TaskStats::changed_with_message("custom outcome"),
            TaskStatus::Changed,
            false,
        ),
        (
            "failed",
            TaskStats::from_counts(1, 2, 3, 4),
            TaskStatus::Failed,
            true,
        ),
    ] {
        conformance(case, |mode, ctx, log| {
            let task = with_recording_metadata(
                TestTask::new(case).returning(Behavior::Return(stats.clone().finish())),
            );
            let summary = mode.run(&[&task], ctx, log, None);
            task.assert_ran(true);
            let entries = log.task_entries();
            assert_eq!(entries.len(), 1);
            let entry = &entries[0];
            assert_recording_metadata(entry, &task);
            assert_eq!(entry.status, status);
            assert_eq!(entry.message_is_summary, is_summary);
            let message = stats
                .message()
                .map_or_else(|| stats.summary(false), str::to_string);
            assert_eq!(entry.message.as_deref(), Some(message.as_str()));
            assert_eq!(
                entry.actions,
                ActionCounts {
                    applied: stats.changed_count(),
                    skipped: stats.skipped_count(),
                    failed: stats.failed_count(),
                    ..ActionCounts::default()
                }
            );
            assert!(entry.duration.is_some());
            assert_eq!(
                summary.failure_count(),
                usize::from(status == TaskStatus::Failed)
            );
            summary
        });
    }
}

#[test]
fn scheduler_skips_preserve_metadata_without_execution_accounting() {
    for cancelled in [false, true] {
        conformance("metadata on scheduler skip", |mode, ctx, log| {
            let root = TestTask::new("root").returning(Behavior::Error);
            let task = with_recording_metadata(TestTask::new("skipped").after(&[&root]));
            if cancelled {
                ctx.cancellation_token().cancel();
            }
            let summary = mode.run(&[&task, &root], ctx, log, None);
            task.assert_ran(false);
            let entries = log.task_entries();
            let entry = entries
                .iter()
                .find(|entry| entry.task_id == task.log_key())
                .unwrap();
            assert_recording_metadata(entry, &task);
            assert_eq!(
                entry.status,
                if cancelled {
                    TaskStatus::Interrupted
                } else {
                    TaskStatus::Blocked
                }
            );
            assert_eq!(
                entry.message.as_deref(),
                Some(if cancelled {
                    "cancelled"
                } else {
                    "blocked by failed dependency: root"
                })
            );
            assert!(!entry.message_is_summary);
            assert_eq!(entry.actions, ActionCounts::default());
            assert!(entry.duration.is_none());
            assert_eq!(summary.failure_count(), usize::from(!cancelled));
            summary
        });
    }
}

#[test]
fn panic_recording_preserves_metadata_without_batch_accounting() {
    conformance("metadata on panic", |mode, ctx, log| {
        let task = with_recording_metadata(TestTask::new("panic").returning(Behavior::PanicStr));
        let summary = mode.run(&[&task], ctx, log, None);
        task.assert_ran(true);
        let entries = log.task_entries();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_recording_metadata(entry, &task);
        assert_eq!(entry.status, TaskStatus::Failed);
        assert_eq!(
            entry.message.as_deref(),
            Some("task panicked: simulated panic")
        );
        assert!(!entry.message_is_summary);
        assert_eq!(entry.actions, ActionCounts::default());
        assert!(entry.duration.is_none());
        assert_eq!(summary.failure_count(), 1);
        summary
    });
}

struct DetailTask;

impl Task for DetailTask {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("detail-task")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        ctx.log()
            .action("install", "demo-package", false, "install demo-package");
        Ok(TaskStats::changed().finish())
    }
}

#[test]
fn stages_precede_stats_and_details_are_not_repeated_in_summary() {
    for verbose in [false, true] {
        conformance_with_verbosity(
            "stage, stats, detail, and timing",
            verbose,
            |mode, ctx, log| {
                let cases = [
                    ("install-symlinks", 37),
                    ("apply-permissions", 3),
                    ("configure-systemd", 2),
                    ("install-hooks", 1),
                ];
                let stats: Vec<_> = cases
                    .iter()
                    .map(|&(name, count)| {
                        TestTask::new(name).returning(Behavior::Return(
                            TaskStats::from_counts(0, count, 0, 0).finish(),
                        ))
                    })
                    .collect();
                let mut inapplicable = TestTask::new("no-stage");
                inapplicable.applicable = false;
                let mut tasks: Vec<&dyn Task> =
                    stats.iter().map(|task| -> &dyn Task { task }).collect();
                let empty = TestTask::new("empty-config").returning(Behavior::Return(
                    TaskResult::NotApplicable("nothing configured".into()),
                ));
                tasks.push(&empty);
                tasks.push(&DetailTask);
                tasks.push(&inapplicable);
                let summary = mode.run(&tasks, ctx, log, None);
                log.print_summary();
                let contents = std::fs::read_to_string(log.log_path().expect("run log")).unwrap();
                for (task, &(_, count)) in stats.iter().zip(&cases) {
                    let stage = format!("[stage] {}", task.name());
                    assert_eq!(contents.matches(&stage).count(), 1, "{stage}: {contents}");
                    let info = format!("0 changed, {count} already ok");
                    assert!(
                        contents.find(&stage).unwrap() < contents.find(&info).expect("stats info"),
                        "{stage} must precede its statistics: {contents}"
                    );
                }
                assert_eq!(
                    contents.matches("install demo-package").count(),
                    1,
                    "task details must not repeat in the final file summary: {contents}"
                );
                assert_eq!(
                    contents.matches("[task_timing] elapsed ").count(),
                    6,
                    "each executed task records one duration: {contents}"
                );
                let empty_stage = "[stage] empty-config";
                assert_eq!(contents.matches(empty_stage).count(), 1);
                assert!(
                    contents.find(empty_stage).unwrap()
                        < contents
                            .find("[task_skip] nothing configured")
                            .expect("empty outcome")
                );
                assert!(
                    !contents.contains("[stage] no-stage"),
                    "inapplicable work has no stage"
                );
                assert!(
                    log.task_entries()
                        .iter()
                        .filter(|entry| entry.status != TaskStatus::NotApplicable)
                        .all(|entry| entry.duration.is_some())
                );
                assert_eq!(summary.failure_count(), 0);
                summary
            },
        );
    }
}

#[test]
fn dependency_block_reason_is_owned_by_recorded_task_result() {
    #[derive(Default)]
    struct RecordingLog {
        messages: std::sync::Mutex<Vec<(MsgKind, String)>>,
        records: std::sync::Mutex<Vec<TaskEntry>>,
    }

    impl logging::Output for RecordingLog {
        fn emit(&self, kind: MsgKind, msg: std::borrow::Cow<'_, str>) {
            self.messages.lock().unwrap().push((kind, msg.into_owned()));
        }
    }

    impl TaskRecorder for RecordingLog {
        fn record_task(&self, task: TaskEntry) {
            self.records.lock().unwrap().push(task);
        }
    }

    let log = RecordingLog::default();
    let task = TestTask::new("blocked");
    record_scheduler_skip(&task, &log, "dependency failed", TaskStatus::Blocked);
    assert_eq!(
        *log.messages.lock().unwrap(),
        [(MsgKind::Debug, "dependency failed".to_string())],
        "keep the reason in the persistent debug log, not a premature info line"
    );
    {
        let records = log.records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, TaskStatus::Blocked);
        assert_eq!(records[0].message.as_deref(), Some("dependency failed"));
        drop(records);
    }
    task.assert_ran(false);
}
