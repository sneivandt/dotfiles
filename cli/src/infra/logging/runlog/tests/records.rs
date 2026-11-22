use super::*;

fn isolated_run_log() -> (RunLog, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run_log = RunLog::new("test", tmp.path(), Instant::now()).expect("run log");
    (run_log, tmp)
}

#[test]
fn run_log_is_created() {
    let (run_log, _tmp) = isolated_run_log();
    assert!(run_log.path().exists(), "run log file should be created");
}

#[test]
fn run_log_has_header() {
    let (run_log, _tmp) = isolated_run_log();
    let contents = fs::read_to_string(run_log.path()).unwrap();
    assert!(
        contents.starts_with("# Dotfiles "),
        "run log should start with header"
    );
    assert!(
        contents.contains("seq | elapsed_us | wall_utc | context | event | message"),
        "header should describe columns"
    );
}

#[test]
fn run_log_emit_writes_event() {
    let (run_log, _tmp) = isolated_run_log();
    let marker = format!("run-log-marker-{}", std::process::id());
    run_log.emit(LogEvent::Info, &marker);
    let contents = fs::read_to_string(run_log.path()).unwrap();
    assert!(contents.contains(&marker), "event should appear in run log");
    assert!(
        contents.contains("[info]"),
        "run-log event should include info event name"
    );
}

#[test]
fn run_log_event_appears_after_context_without_padding() {
    let (run_log, _tmp) = isolated_run_log();
    run_log.emit(LogEvent::Warn, "event-order");
    let contents = fs::read_to_string(run_log.path()).unwrap();
    let line = contents
        .lines()
        .find(|l| l.contains("event-order"))
        .unwrap();
    assert!(
        line.contains("] [warn] event-order"),
        "event should be bracketed immediately before the message: {line}"
    );
    assert!(
        !line.contains("[warn]  event-order"),
        "event column should not add padding after the closing bracket: {line}"
    );
}

#[test]
fn run_log_emit_has_microsecond_precision() {
    let (run_log, _tmp) = isolated_run_log();
    run_log.emit(LogEvent::Stage, "precision-test");
    let contents = fs::read_to_string(run_log.path()).unwrap();
    let line = contents
        .lines()
        .find(|line| line.ends_with("[stage] precision-test"))
        .unwrap();
    let stamp = line
        .split_whitespace()
        .find(|part| part.contains('T') && part.ends_with('Z'))
        .unwrap();
    assert_eq!(
        stamp.len(),
        27,
        "persisted wall time must include six fractional digits: {stamp}"
    );
    for (index, byte) in stamp.bytes().enumerate() {
        assert!(
            match index {
                4 | 7 => byte == b'-',
                10 => byte == b'T',
                13 | 16 => byte == b':',
                19 => byte == b'.',
                26 => byte == b'Z',
                _ => byte.is_ascii_digit(),
            },
            "invalid timestamp byte {index}: {stamp}"
        );
    }
}

#[test]
fn run_log_emit_task_includes_task_name() {
    let (run_log, _tmp) = isolated_run_log();
    run_log.emit_task(LogEvent::TaskStart, "Install symlinks", "deps satisfied");
    let contents = fs::read_to_string(run_log.path()).unwrap();
    assert!(
        contents.contains("[Install symlinks]"),
        "run-log task event should include task name in brackets"
    );
    assert!(
        contents.contains("deps satisfied"),
        "run-log task event should include the message"
    );
}

#[test]
fn explicit_task_event_preserves_context() {
    let (run_log, _tmp) = isolated_run_log();
    run_log.emit_task(LogEvent::TaskStart, "typed-task", "complete");
    let contents = fs::read_to_string(run_log.path()).unwrap();
    assert!(contents.contains("[typed-task] [task_start] complete"));
}

#[test]
fn poisoned_file_lock_marks_run_log_unhealthy() {
    let (run_log, _tmp) = isolated_run_log();
    let run_log = std::sync::Arc::new(run_log);
    let poisoner = std::sync::Arc::clone(&run_log);
    drop(
        std::thread::spawn(move || {
            let _guard = poisoner.file.lock().unwrap();
            panic!("poison run-log lock");
        })
        .join(),
    );

    run_log.emit(LogEvent::Info, "after poison");

    assert!(!run_log.is_healthy());
}

#[test]
fn recovery_hint_disappears_when_persistent_storage_degrades() {
    use crate::infra::logging::Output as _;
    let (log, _tmp, _guard) = crate::infra::logging::isolated_logger();
    crate::app::run::recovery_hint(&log);
    assert!(
        log.captured_lines()
            .last()
            .unwrap()
            .starts_with("  Run 'dotfiles log --id ")
    );
    let run = log.run_log().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _file_guard = run.file.lock().unwrap();
        panic!("fixture log failure");
    }));
    assert!(result.is_err());
    run.emit(LogEvent::Info, "mark the poisoned sink degraded");
    assert!(!run.is_healthy());
    let before = log.captured_lines();
    crate::app::run::recovery_hint(&log);
    assert_eq!(log.captured_lines(), before);
}

#[test]
fn run_log_resource_events_include_messages() {
    let (run_log, _tmp) = isolated_run_log();
    run_log.emit(LogEvent::ResourceCheck, "~/.bashrc state=Missing");
    run_log.emit(LogEvent::ResourceApply, "link ~/.bashrc");
    run_log.emit(LogEvent::ResourceResult, "~/.bashrc applied");
    let contents = fs::read_to_string(run_log.path()).unwrap();
    assert!(contents.contains("~/.bashrc state=Missing"));
    assert!(contents.contains("link ~/.bashrc"));
    assert!(contents.contains("~/.bashrc applied"));
}

#[test]
fn run_log_events_are_chronologically_ordered() {
    let (run_log, _tmp) = isolated_run_log();
    run_log.emit(LogEvent::Stage, "first");
    run_log.emit(LogEvent::Info, "second");
    let contents = fs::read_to_string(run_log.path()).unwrap();
    let first_pos = contents.find("first").expect("first in log");
    let second_pos = contents.find("second").expect("second in log");
    assert!(
        first_pos < second_pos,
        "events should appear in chronological order"
    );
}

#[test]
fn log_event_names_are_stable_snake_case() {
    assert_eq!(LogEvent::Debug.name(), "debug");
    assert_eq!(LogEvent::Info.name(), "info");
    assert_eq!(LogEvent::Stage.name(), "stage");
    assert_eq!(LogEvent::TaskStart.name(), "task_start");
    assert_eq!(LogEvent::Summary.name(), "summary");
    assert_eq!(LogEvent::ResourceApply.name(), "resource_apply");
    assert_eq!(LogEvent::Warn.name(), "warn");
    assert_eq!(LogEvent::Error.name(), "error");
}

#[test]
fn log_thread_name_returns_nonempty() {
    let name = log_thread_name();
    assert!(
        !name.is_empty(),
        "log_thread_name should never return empty string"
    );
}

#[test]
fn set_log_thread_name_is_retrieved_on_unnamed_thread() {
    let result = std::thread::spawn(|| {
        set_log_thread_name("my-task");
        log_thread_name()
    })
    .join()
    .expect("thread should not panic");
    assert_eq!(result, "my-task");
}

#[test]
fn log_task_context_restores_previous_name() {
    let result = std::thread::spawn(|| {
        set_log_thread_name("outer-task");
        let inner = {
            let _guard = log_task_context("inner-task");
            log_thread_name()
        };
        (inner, log_thread_name())
    })
    .join()
    .expect("thread should not panic");
    assert_eq!(result, ("inner-task".to_string(), "outer-task".to_string()));
}

#[test]
fn run_log_omits_blank_messages() {
    let (run_log, _tmp) = isolated_run_log();
    run_log.emit_task(LogEvent::TaskStart, "task-name", "");
    run_log.emit(LogEvent::Debug, "   \t");
    run_log.emit(LogEvent::Debug, "\x1b[32m \t\x1b[0m");
    run_log.emit(LogEvent::Info, "after blanks");
    let contents = fs::read_to_string(run_log.path()).unwrap();
    assert!(
        !contents.contains("[task-name]"),
        "empty messages should be omitted"
    );
    assert!(
        contents
            .lines()
            .any(|line| line.starts_with("000001 ") && line.contains("after blanks")),
        "blank messages should not consume sequence numbers"
    );
}

#[test]
fn run_log_trims_single_line_messages_without_changing_internal_spacing() {
    let (run_log, _tmp) = isolated_run_log();
    run_log.emit_task(
        LogEvent::Info,
        "fixture",
        " \t\x1b[32mkeep  inner\tspace\x1b[0m \t",
    );

    let contents = fs::read_to_string(run_log.path()).unwrap();
    assert!(
        contents
            .lines()
            .last()
            .unwrap()
            .ends_with("[fixture] [info] keep  inner\tspace")
    );
    assert!(!contents.contains('\x1b'));
}

#[test]
fn run_log_preserves_multiline_message_without_splitting_records() {
    let (run_log, _tmp) = isolated_run_log();
    run_log.emit(LogEvent::Info, "first\n\n  second  ");
    let contents = fs::read_to_string(run_log.path()).unwrap();
    let line = contents
        .lines()
        .find(|line| line.contains("first"))
        .unwrap();
    let stored = StoredRecord::from_line(line).unwrap();
    assert!(
        matches!(stored.record, Record::Message { text, .. } if text == "first\n\n  second  "),
        "multiline messages must retain indentation, blank lines and trailing spaces"
    );
    assert!(
        !contents.lines().any(str::is_empty),
        "run-log log should not contain blank lines"
    );
}

#[test]
fn run_log_preserves_contexts_with_record_delimiters_and_line_breaks() {
    for context in [
        "overlay:before] [after",
        "overlay:first\r\nsecond",
        "script\0path",
    ] {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit_task(LogEvent::Info, context, " \x1b[32msingle line\x1b[0m ");
        run_log.emit_task(LogEvent::Info, context, " \t ");
        run_log.emit_task(LogEvent::Warn, context, "first\nsecond");
        run_log.record_in_context(
            context,
            Record::TaskDuration {
                task_id: context.into(),
                elapsed_us: 42,
            },
        );

        let content = fs::read_to_string(run_log.path()).unwrap();
        assert!(!content.contains('\0'), "the log must remain a text file");
        let lines: Vec<_> = content
            .lines()
            .filter(|line| !line.starts_with('#'))
            .collect();
        assert_eq!(lines.len(), 3, "contexts must not split log entries");
        let records: Vec<_> = lines
            .into_iter()
            .map(|line| StoredRecord::from_line(line).expect("a complete structured record"))
            .collect();
        assert!(records.iter().all(|record| record.context == context));
        assert!(matches!(&records[0].record, Record::Message { event, text }
                    if event == "info" && text == "single line"));
        assert!(matches!(&records[1].record, Record::Message { text, .. }
                    if text == "first\nsecond"));
        assert!(
            matches!(&records[2].record, Record::TaskDuration { task_id, elapsed_us }
                    if task_id == context && *elapsed_us == 42)
        );
    }
}

#[test]
fn nul_in_single_line_message_is_retained_without_binary_log_bytes() {
    let (log, _tmp) = isolated_run_log();
    log.emit(LogEvent::Debug, "one\0two");
    let contents = fs::read_to_string(log.path()).unwrap();
    assert!(!contents.contains('\0'));
    let stored = contents.lines().find_map(StoredRecord::from_line).unwrap();
    assert!(matches!(stored.record, Record::Message { text, .. } if text == "one\0two"));
}

#[test]
fn concurrent_events_keep_complete_records_sequence_numbers_and_thread_contexts() {
    let (run_log, _tmp) = isolated_run_log();
    let workers = 4;
    let events = 16;
    let barrier = std::sync::Barrier::new(workers);
    std::thread::scope(|scope| {
        for worker in 0..workers {
            let run_log = &run_log;
            let barrier = &barrier;
            scope.spawn(move || {
                let _context = log_task_context(&format!("worker-{worker}"));
                barrier.wait();
                for event in 0..events {
                    run_log.emit(LogEvent::Info, &format!("{event}\nretained indentation"));
                }
            });
        }
    });
    let contents = fs::read_to_string(run_log.path()).unwrap();
    let lines: Vec<_> = contents
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect();
    assert_eq!(lines.len(), workers * events);
    let mut seen = std::collections::BTreeMap::<String, usize>::new();
    for (index, line) in lines.into_iter().enumerate() {
        assert_eq!(
            line.split_whitespace()
                .next()
                .unwrap()
                .parse::<usize>()
                .unwrap(),
            index + 1,
            "sequence allocation and writing must be serialized"
        );
        let stored = StoredRecord::from_line(line).expect("a complete structured record");
        let event_index = seen.entry(stored.context.clone()).or_default();
        let Record::Message { event, text } = stored.record else {
            panic!("expected a multiline message");
        };
        assert_eq!(event, "info");
        assert_eq!(text, format!("{event_index}\nretained indentation"));
        *event_index += 1;
    }
    assert_eq!(
        seen,
        (0..workers)
            .map(|worker| (format!("worker-{worker}"), events))
            .collect(),
        "each worker retains its own context and emission order"
    );
    assert!(run_log.is_healthy());
}

#[test]
fn run_log_strips_ansi_from_message() {
    let (run_log, _tmp) = isolated_run_log();
    run_log.emit(LogEvent::Info, "\x1b[31mred-message\x1b[0m");
    let contents = fs::read_to_string(run_log.path()).unwrap();
    assert!(
        contents.contains("red-message"),
        "stripped text should appear"
    );
    assert!(
        !contents.contains("\x1b[31m"),
        "ANSI codes should be stripped"
    );
}
#[test]
fn lifecycle_preserves_parent_and_records_one_finish() {
    let (log, _tmp) = isolated_run_log();
    log.start_run("install", Some("parent-id".into()));
    log.finish(RunOutcome::Failed, 1);
    log.finish(RunOutcome::Succeeded, 0);
    let content = fs::read_to_string(log.path()).unwrap();
    let records: Vec<_> = content
        .lines()
        .filter_map(StoredRecord::from_line)
        .collect();
    assert!(
        matches!(&records[0].record, Record::RunStart { run_id, parent_run_id, .. }
            if *run_id == log.id() && parent_run_id.as_deref() == Some("parent-id"))
    );
    assert!(matches!(
        records[1].record,
        Record::RunFinish {
            outcome: RunOutcome::Failed,
            exit_code: 1,
            ..
        }
    ));
    assert_eq!(records.len(), 2);
}

#[test]
fn degraded_persistence_warns_once_in_a_fresh_process() {
    const CHILD: &str = "DOTFILES_TEST_DEGRADED_LOG_WARNING";
    if std::env::var_os(CHILD).is_some() {
        warn_degraded("first failure");
        warn_degraded("repeat failure");
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "infra::logging::runlog::tests::records::degraded_persistence_warns_once_in_a_fresh_process",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        stderr
            .matches("persistent run logging is unavailable")
            .count(),
        1,
        "{stderr}"
    );
    assert!(stderr.contains("first failure"), "{stderr}");
}
