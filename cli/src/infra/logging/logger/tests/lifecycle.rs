use super::*;

#[test]
fn structured_lifecycle_has_identity_and_finishes_once_without_a_file_log() {
    let mut log = Logger::build("update", None, Instant::now(), false);
    log.set_output_mode(super::super::super::OutputMode::Jsonl);
    log.start_run(Some("parent-fixture".into()));
    let id = log.run_id().expect("stream identity");
    assert_eq!(log.finish_run(130), 130);
    assert_eq!(log.finish_run(130), 130);
    let records: Vec<serde_json::Value> = log
        .captured_lines()
        .iter()
        .map(|line| serde_json::from_str(line.trim()).expect("valid JSON record"))
        .collect();
    assert_eq!(records.len(), 2);
    assert_eq!(records.first().unwrap()["run_id"], id);
    assert_eq!(records.first().unwrap()["parent_run_id"], "parent-fixture");
    assert_eq!(records.last().unwrap()["run_id"], id);
    assert_eq!(records.last().unwrap()["outcome"], "interrupted");
    assert_eq!(records.last().unwrap()["exit_code"], 130);
}

fn task_entry(
    name: &str,
    status: TaskStatus,
    message: Option<&str>,
    actions: ActionCounts,
) -> TaskEntry {
    TaskEntry::new(
        name,
        name,
        status,
        message,
        actions,
        TaskVisibility::Visible,
    )
}

#[test]
fn logger_new() {
    let (mut log, _tmp, _guard) = isolated_logger();
    assert!(log.task_entries().is_empty(), "expected empty task list");
    assert!(!log.is_quiet());
    log.set_output_mode(super::super::super::OutputMode::Quiet);
    assert!(log.is_quiet());
}

#[test]
fn task_records_persist_all_outcomes_and_actions_through_the_log_trait() {
    use crate::infra::logging::records::{Record, StoredRecord};
    let (log, _tmp, _guard) = isolated_logger();
    let log_ref: &dyn Log = &log;
    let actions = ActionCounts {
        applied: 4,
        planned: 2,
        skipped: 1,
        failed: 3,
        interrupted: 6,
        not_attempted: 5,
    };
    let cases = [
        (TaskStatus::Ok, None),
        (TaskStatus::Changed, Some("updated")),
        (TaskStatus::Skipped, Some("not on arch")),
        (TaskStatus::Failed, Some("permission denied")),
        (TaskStatus::DryRun, Some("would update")),
    ];
    for (index, (status, message)) in cases.iter().enumerate() {
        log_ref.record_task(
            TaskEntry::new(
                format!("task-{index}"),
                "shared display name",
                *status,
                *message,
                actions,
                TaskVisibility::Visible,
            )
            .with_selector("example"),
        );
    }
    let content = fs::read_to_string(log_ref.run_log().unwrap().path()).unwrap();
    let records: Vec<_> = content
        .lines()
        .filter_map(StoredRecord::from_line)
        .collect();
    assert_eq!(
        records.len(),
        cases.len(),
        "persist each result exactly once"
    );
    let tasks = log.task_entries();
    assert_eq!(tasks.len(), cases.len());
    for (index, ((expected_status, expected_reason), stored)) in
        cases.into_iter().zip(records).enumerate()
    {
        let expected_id = format!("task-{index}");
        assert_eq!(stored.context, expected_id);
        let Record::TaskResult {
            task_id,
            selector,
            name,
            status,
            reason,
            actions: recorded,
        } = stored.record
        else {
            panic!("expected task result");
        };
        assert_eq!(task_id, expected_id);
        assert_eq!(selector.as_deref(), Some("example"));
        assert_eq!(name, "shared display name");
        assert_eq!(status, expected_status);
        assert_eq!(reason.as_deref(), expected_reason);
        assert_eq!(recorded, actions);
        assert_eq!(tasks[index].task_id, task_id);
        assert_eq!(tasks[index].status, status);
        assert_eq!(tasks[index].message, reason);
        assert_eq!(tasks[index].actions, recorded);
    }
}

#[test]
fn task_durations_use_scheduler_identity_not_display_name() {
    use crate::infra::logging::records::{Record, StoredRecord};
    use std::time::Duration;
    let (log, _tmp, _guard) = isolated_logger();
    for id in ["first", "second"] {
        log.record_task(TaskEntry::new(
            id,
            "same name",
            TaskStatus::Ok,
            None,
            ActionCounts::default(),
            TaskVisibility::Visible,
        ));
    }
    let log_ref: &dyn Log = &log;
    log_ref.record_task_duration("first", Duration::from_micros(1234));
    log_ref.record_task_duration("missing", Duration::from_micros(9999));
    let tasks = log.task_entries();
    assert_eq!(tasks[0].duration, Some(Duration::from_micros(1234)));
    assert_eq!(tasks[1].duration, None);
    let content = fs::read_to_string(log.log_path().unwrap()).unwrap();
    let durations: Vec<_> = content
        .lines()
        .filter_map(StoredRecord::from_line)
        .filter_map(|stored| {
            if let Record::TaskDuration {
                task_id,
                elapsed_us,
            } = stored.record
            {
                assert_eq!(stored.context, task_id);
                Some((task_id, elapsed_us))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        durations,
        [("first".into(), 1234), ("missing".into(), 9999)]
    );
}

#[test]
fn log_file_is_created() {
    let (log, _tmp, _guard) = isolated_logger();
    let path = log.log_path().expect("log path should exist");
    assert!(path.exists(), "log file should be created on Logger::new");
}

#[test]
fn failure_count_returns_correct_count() {
    let (log, _tmp, _guard) = isolated_logger();
    assert_eq!(log.failure_count(), 0);
    log.record_task(task_entry(
        "a",
        TaskStatus::Ok,
        None,
        ActionCounts::default(),
    ));
    log.record_task(task_entry(
        "b",
        TaskStatus::Failed,
        Some("error 1"),
        ActionCounts::default(),
    ));
    log.record_task(task_entry(
        "c",
        TaskStatus::Failed,
        Some("error 2"),
        ActionCounts::default(),
    ));
    log.record_task(task_entry(
        "d",
        TaskStatus::Skipped,
        None,
        ActionCounts::default(),
    ));
    assert_eq!(log.failure_count(), 2);
}

#[test]
fn summary_does_not_report_the_persistent_log_path() {
    let (log, _tmp, _guard) = isolated_logger();
    log.record_task(task_entry(
        "summary-test",
        TaskStatus::Ok,
        None,
        ActionCounts::default(),
    ));
    log.print_summary(super::super::super::CompletionDisposition::Succeeded);
    let path = log.log_path().expect("log path");
    let contents = fs::read_to_string(path).unwrap();
    assert!(
        !contents.contains(&format!("Log: {}", path.display())),
        "summary should not expose the persistent log path: {contents}"
    );
    assert!(
        !contents.contains("Summary"),
        "file summary should not include the full task breakdown: {contents}"
    );
    assert_eq!(
        contents
            .lines()
            .filter_map(super::super::super::records::StoredRecord::from_line)
            .filter(|record| matches!(
                record.record,
                super::super::super::records::Record::TaskResult { .. }
            ))
            .count(),
        1,
        "task facts are recorded once; the summary must not repeat them"
    );
    assert!(
        contents
            .lines()
            .any(|line| line.ends_with("[info] No checks ran")),
        "file summary should report the counts without repeating the process duration: {contents}"
    );
    let lower_contents = contents.to_lowercase();
    assert!(
        !lower_contents.contains("not run") && !lower_contents.contains("checks:"),
        "test summary should omit unchanged checks: {contents}"
    );
}

#[test]
fn invocation_clock_accounts_for_elapsed_time_before_logger_setup() {
    let before = SystemTime::now();
    let start = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
    let log = Logger::build("update", None, start, false);
    assert!(log.invocation_start() < before);
}

#[test]
fn run_log_health_reflects_the_availability_of_persistent_storage() {
    let (log, _tmp, _guard) = isolated_logger();
    assert!(log.run_log_is_healthy());
    assert!(!Logger::test().run_log_is_healthy());
}
