//! Scheduler-owned stage ordering and result recording (not console styling).

use super::*;
use crate::infra::logging::{MsgKind, TaskRecorder};

struct DetailTask;

impl Task for DetailTask {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("detail-task")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        ctx.log().info("installed: demo-package");
        Ok(TaskStats::changed_with_message("1 changed, 0 already ok").finish())
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
                            TaskStats {
                                already_ok: count,
                                ..TaskStats::default()
                            }
                            .finish(),
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
                    contents.matches("installed: demo-package").count(),
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
