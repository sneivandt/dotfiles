use super::*;
fn record(context: &str, record: Record) -> String {
    format!(
        "000001 +          10 2026-09-07T10:00:00.000000Z [{context}] [record] {}\n",
        serde_json::to_string(&StoredRecord::new(context, record)).unwrap()
    )
}

#[test]
fn metadata_skips_payload_shapes_and_incomplete_facts_do_not_hide_legacy_lines() {
    let command = r#"000001 +10 stamp [task-a] [record] {"schema":1,"context":"task-a","type":"command","outcome":"output_limit","stdout":{"ignored":[1,2,3]}}"#;
    let metadata = RecordMetadata::from_line(command).unwrap();
    assert_eq!(metadata.kind, "command");
    assert_eq!(metadata.context, "task-a");
    let mut raw = Vec::new();
    write_selected_contents(command, false, true, Some("task-a"), &mut raw).unwrap();
    assert_eq!(raw, format!("{command}\n").as_bytes());

    let content = concat!(
        "000001 +10 stamp [task-a] [task_done] retain this outcome\n",
        "000002 +20 stamp [task-a] [record] {\"schema\":1,\"context\":\"task-a\",\"type\":\"task_result\"}\n",
        "000003 +30 stamp [task-a] [task_timing] retain this timing\n",
        "000004 +40 stamp [task-a] [record] {\"schema\":1,\"context\":\"task-a\",\"type\":\"task_duration\"}\n",
    );
    let mut output = Vec::new();
    write_selected_contents(content, true, false, None, &mut output).unwrap();
    assert_eq!(output, content.as_bytes());
}

#[test]
fn human_log_escapes_nuls_without_changing_raw_output_or_task_selection() {
    let task_key = "Pause\0new.sh";
    let content = record(
        task_key,
        Record::TaskStart {
            selector: "script-pause".into(),
            name: "Pause".into(),
        },
    ) + &record(
        task_key,
        Record::Message {
            event: "warn".into(),
            text: "one\0two".into(),
        },
    ) + "000003 +30 stamp [Pause\0new.sh] [warn] old\0text\n";
    for verbose in [false, true] {
        let mut out = Vec::new();
        write_selected_contents(&content, verbose, false, Some("script-pause"), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains('\0'));
        assert!(
            text.contains(r"one\0two") && text.contains(r"old\0text"),
            "{text}"
        );
    }
    let mut raw = Vec::new();
    write_selected_contents(&content, false, true, Some("script-pause"), &mut raw).unwrap();
    assert_eq!(raw, content.as_bytes());
}

#[test]
fn streamed_logs_preserve_input_and_output_errors() {
    struct BrokenOutput;
    impl std::io::Write for BrokenOutput {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "fixture output failure",
            ))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    for raw in [false, true] {
        let error = write_selected_contents("legacy record\n", false, raw, None, &mut BrokenOutput)
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert!(format!("{error:#}").contains("fixture output failure"));
    }
    let mut invalid = std::io::Cursor::new(b"valid\n\xff\n");
    let error = write_selected_log(&mut invalid, false, false, None, &mut Vec::new()).unwrap_err();
    assert_eq!(error.to_string(), "reading log index");
    assert_eq!(
        error.downcast_ref::<std::io::Error>().unwrap().kind(),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn history_and_stable_selection_survive_newer_runs_and_missing_finish() {
    let tmp = tempfile::tempdir().unwrap();
    let id = "20260907T100000Z-install-1";
    let content = record(
        "main",
        Record::RunStart {
            run_id: id.into(),
            command: "install".into(),
            parent_run_id: Some("parent-id".into()),
        },
    ) + &record(
        "main",
        Record::RunContext {
            profile: "desktop".into(),
            platform: "Linux".into(),
            dry_run: true,
        },
    ) + &record(
        "main",
        Record::RunFinish {
            outcome: RunOutcome::Failed,
            elapsed_us: 1_200_000,
            exit_code: 1,
        },
    );
    write_run(tmp.path(), &format!("{id}.log"), &content);
    write_run(
        tmp.path(),
        "20260907T100001Z-install-2.log",
        &record(
            "main",
            Record::RunStart {
                run_id: "newer".into(),
                command: "install".into(),
                parent_run_id: None,
            },
        ),
    );
    write_run(tmp.path(), "20260906T100000Z-check-3.log", "legacy log\n");
    let listing = capture(
        tmp.path(),
        &LogOpts {
            list: true,
            ..opts()
        },
        false,
    );
    for expected in [
        "failed",
        "unfinished",
        "unknown",
        "desktop",
        "1.2s",
        id,
        "parent-id",
    ] {
        assert!(listing.contains(expected), "missing {expected}: {listing}");
    }
    let selected = capture(
        tmp.path(),
        &LogOpts {
            id: Some(id.into()),
            raw: true,
            ..opts()
        },
        false,
    );
    assert_eq!(selected, content);
    let invalid = run_in_dir(
        tmp.path(),
        &LogOpts {
            id: Some("../not-a-log".into()),
            ..opts()
        },
        false,
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(invalid.to_string().contains("No retained run"));
}

#[test]
fn failed_command_and_successful_stderr_are_visible_without_verbose() {
    let command = |outcome: &str, stderr: &str| Record::Command {
        command: "example".into(),
        outcome: outcome.into(),
        exit_code: Some(i32::from(outcome == "failed")),
        elapsed_us: 10,
        stdout: Some("first\n  second\n\nlast".into()),
        stderr: Some(stderr.into()),
        stdout_bytes: 20,
        stderr_bytes: stderr.len(),
    };
    let content = record("task-a", command("failed", "cause"))
        + &record("task-b", command("succeeded", "warning"));
    let mut out = Vec::new();
    write_selected_contents(&content, false, false, None, &mut out).unwrap();
    let output = String::from_utf8(out).unwrap();
    for expected in ["first\n  second\n\nlast", "cause", "warning", "exit 1"] {
        assert!(output.contains(expected), "{output}");
    }
    let mut selected = Vec::new();
    write_selected_contents(&content, false, false, Some("task-a"), &mut selected).unwrap();
    let selected = String::from_utf8(selected).unwrap();
    assert!(selected.contains("cause"));
    assert!(!selected.contains("warning"));
}

#[test]
fn readable_command_output_reports_omitted_bytes_and_hides_empty_streams() {
    let command = |stdout: Option<&str>, stdout_bytes| Record::Command {
        command: "fixture-command".into(),
        outcome: "failed".into(),
        exit_code: Some(1),
        elapsed_us: 10,
        stdout: stdout.map(str::to_string),
        stderr: Some(String::new()),
        stdout_bytes,
        stderr_bytes: 0,
    };
    for (record_value, lines) in [
        (command(Some("data"), 4), "\nstdout:\ndata"),
        (command(None, 4), "\nstdout: 4 bytes omitted"),
        (command(None, 0), ""),
        (command(Some(""), 0), ""),
    ] {
        let content = record("fixture-task", record_value);
        let mut output = Vec::new();
        write_selected_contents(&content, false, false, None, &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("fixture-command"), "{output}");
        assert!(!output.contains("stderr"), "{output}");
        assert_eq!(
            output.trim_end().split_once('\n').map(|(_, text)| text),
            lines.strip_prefix('\n'),
            "{output}"
        );
    }
}

#[test]
fn task_selector_resolves_diagnostics_before_a_result_is_recorded() {
    let content = record(
        "internal-id",
        Record::TaskStart {
            selector: "repository".into(),
            name: "Dotfiles repository".into(),
        },
    ) + "000002 +10 stamp [internal-id] [debug] fetching upstream\n"
        + "000003 +11 stamp [another-task] [warn] unrelated\n";
    for selector in ["repository", "internal-id"] {
        let mut out = Vec::new();
        write_selected_contents(&content, true, false, Some(selector), &mut out).unwrap();
        let output = String::from_utf8(out).unwrap();
        assert!(
            output.contains("[task_start] Dotfiles repository"),
            "{output}"
        );
        assert!(output.contains("fetching upstream"), "{output}");
        assert!(!output.contains("unrelated"), "{output}");
    }
}

#[test]
fn normal_view_hides_plumbing_and_verbose_deduplicates_legacy_facts() {
    use std::fmt::Write as _;
    let mut content = String::new();
    for event in [
        "stage",
        "task_start",
        "task_wait",
        "resource_check",
        "debug",
        "trace",
        "summary",
    ] {
        writeln!(
            content,
            "000001 +10 stamp [task-a] [{event}] plumbing-{event}"
        )
        .unwrap();
    }
    for event in ["task_done", "task_skip", "task_timing"] {
        writeln!(
            content,
            "000001 +10 stamp [task-a] [{event}] duplicate-{event}"
        )
        .unwrap();
    }
    content.push_str(&record(
        "task-a",
        Record::TaskResult {
            task_id: "task-a".into(),
            selector: Some("demo".into()),
            name: "Demo task".into(),
            status: TaskStatus::Changed,
            reason: Some("changed fixture".into()),
            actions: crate::infra::logging::ActionCounts::default(),
        },
    ));
    content.push_str(&record(
        "task-a",
        Record::TaskDuration {
            task_id: "task-a".into(),
            elapsed_us: 1_000_000,
        },
    ));
    content.push_str("000001 +10 stamp [legacy] [task_done] legacy-outcome\n");
    content.push_str("000001 +10 stamp [main] [record] {\"schema\":99,\"type\":\"future\"}\n");
    for verbose in [false, true] {
        let mut out = Vec::new();
        write_selected_contents(&content, verbose, false, None, &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("Demo task: Changed: changed fixture"));
        assert!(out.contains("legacy-outcome"));
        assert!(out.contains("future"), "unknown records remain readable");
        assert!(!out.contains("duplicate-"));
        assert_eq!(out.contains("plumbing-"), verbose);
        assert_eq!(out.matches("[task_timing]").count(), usize::from(verbose));
    }
    let mut raw = Vec::new();
    write_selected_contents(&content, false, true, None, &mut raw).unwrap();
    assert_eq!(String::from_utf8(raw).unwrap(), content);
    let mut selected = Vec::new();
    write_selected_contents(&content, false, false, Some("demo"), &mut selected).unwrap();
    let selected = String::from_utf8(selected).unwrap();
    assert!(selected.contains("Demo task"));
    assert!(!selected.contains("legacy-outcome"));
}

#[test]
fn nonzero_probe_diagnostics_are_verbose_unless_stderr_needs_attention() {
    for stderr in ["", "probe warning"] {
        let record = || Record::Command {
            command: "fixture-probe".into(),
            outcome: "nonzero_exit".into(),
            exit_code: Some(1),
            elapsed_us: 100,
            stdout: Some("disabled\n".into()),
            stderr: Some(stderr.into()),
            stdout_bytes: 9,
            stderr_bytes: stderr.len(),
        };
        assert_eq!(render_record(record(), false).is_some(), !stderr.is_empty());
        let (_, verbose) = render_record(record(), true).unwrap();
        assert!(verbose.contains("nonzero exit (unchecked), exit=1"));
        assert!(verbose.contains("disabled\n"));
    }
}

#[test]
fn scope_records_are_readable_with_legacy_context_records() {
    let old = "000001 +10 stamp [main] [record] {\"schema\":1,\"context\":\"main\",\"type\":\"run_context\",\"profile\":\"desktop\",\"platform\":\"Linux\",\"dry_run\":true}\n";
    let content = old.to_owned()
        + &record(
            "main",
            Record::RunScope {
                only: vec!["symlinks".into()],
                skip: vec!["packages".into()],
                with_deps: true,
                no_repo_update: true,
            },
        );
    let mut out = Vec::new();
    write_selected_contents(&content, false, false, None, &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(
        out.contains("Configuration: desktop profile · Linux · dry run"),
        "{out}"
    );
    assert!(out.contains("Selection: symlinks · excluding packages · including dependencies · repository sync disabled"), "{out}");
    let mut verbose = Vec::new();
    write_selected_contents(&content, true, false, None, &mut verbose).unwrap();
    let verbose = String::from_utf8(verbose).unwrap();
    assert!(verbose.contains("[run_context] desktop profile · Linux · dry run"));
    assert!(verbose.contains("[run_scope] symlinks · excluding packages · including dependencies · repository sync disabled"));
}

#[test]
fn task_selector_resolves_to_the_stored_stable_identity() {
    let task_id = "dotfiles_cli::domains::files::symlinks::InstallSymlinks";
    let content = record(
        task_id,
        Record::Message {
            event: "info".into(),
            text: "selected detail".into(),
        },
    ) + &record(
        task_id,
        Record::TaskResult {
            task_id: task_id.into(),
            selector: Some("symlinks".into()),
            name: "Home symlinks".into(),
            status: TaskStatus::Changed,
            reason: None,
            actions: crate::infra::logging::ActionCounts::default(),
        },
    ) + &record(
        "other-task",
        Record::Message {
            event: "info".into(),
            text: "other detail".into(),
        },
    );
    let mut selected = Vec::new();
    write_selected_contents(&content, false, false, Some("symlinks"), &mut selected).unwrap();
    let selected = String::from_utf8(selected).unwrap();
    assert!(selected.contains("selected detail"), "{selected}");
    assert!(selected.contains("Home symlinks"), "{selected}");
    assert!(!selected.contains("other detail"), "{selected}");
    assert!(!selected.contains(task_id), "{selected}");
    assert!(!selected.contains("000001 +"), "{selected}");
    assert!(
        selected.contains("Home symlinks: selected detail"),
        "{selected}"
    );

    let mut verbose = Vec::new();
    write_selected_contents(&content, true, false, Some("symlinks"), &mut verbose).unwrap();
    let verbose = String::from_utf8(verbose).unwrap();
    assert!(verbose.contains(task_id), "{verbose}");
    assert!(verbose.contains("000001 +"), "{verbose}");
}

#[test]
fn human_task_states_do_not_rewrite_raw_records() {
    for (status, label) in [
        (TaskStatus::Ok, "Unchanged"),
        (TaskStatus::DryRun, "Planned"),
        (TaskStatus::NotApplicable, "Not applicable"),
        (TaskStatus::Blocked, "Blocked"),
        (TaskStatus::Interrupted, "Interrupted"),
    ] {
        let contents = record(
            "task",
            Record::TaskResult {
                task_id: "task".into(),
                selector: None,
                name: "Example".into(),
                status,
                reason: Some("original reason".into()),
                actions: crate::infra::logging::ActionCounts::default(),
            },
        );
        let mut human = Vec::new();
        write_selected_contents(&contents, false, false, None, &mut human).unwrap();
        assert!(
            String::from_utf8(human)
                .unwrap()
                .contains(&format!("Example: {label}: original reason"))
        );
        let mut raw = Vec::new();
        write_selected_contents(&contents, false, true, None, &mut raw).unwrap();
        assert_eq!(String::from_utf8(raw).unwrap(), contents);
    }
}

#[test]
fn readable_logs_name_unfinished_tasks_and_plans_without_storage_metadata() {
    let task = "dotfiles_cli::example::PendingTask";
    let contents =
        "# Dotfiles test\n# Columns: seq | elapsed_us | wall_utc | context | event | message\n"
            .to_owned()
            + &record(
                task,
                Record::Message {
                    event: "task_start".into(),
                    text: "Pending work".into(),
                },
            )
            + &record(
                task,
                Record::Action {
                    verb: "link".into(),
                    subject: "example".into(),
                    planned: true,
                    message: "link example".into(),
                },
            )
            + &record(
                task,
                Record::Message {
                    event: "warn".into(),
                    text: "fixture warning\nsecond line".into(),
                },
            )
            + &format!(
                "000002 +20 now [{task}] [info] ordinary message\n\
                     000003 +30 now [{task}] [warn] ordinary warning\n  continuation\n"
            );
    let mut output = Vec::new();
    write_selected_contents(&contents, false, false, None, &mut output).unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "# Dotfiles test\nPending work · Planned: link example\nPending work · Warning: fixture warning\nsecond line\nPending work: ordinary message\nPending work · Warning: ordinary warning\n  continuation\n",
    );
    let mut raw = Vec::new();
    write_selected_contents(&contents, false, true, None, &mut raw).unwrap();
    assert_eq!(raw, contents.as_bytes());
}

#[test]
fn unknown_schema_and_malformed_records_remain_visible() {
    let content =
        "000001 +10 now [task] [record] {\"schema\":99}\n000002 +20 now [task] [record] {broken\n";
    let mut output = Vec::new();
    write_selected_contents(content, false, false, None, &mut output).unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), content);
    assert!(
        StoredRecord::from_line("000001 +10 now [task] [info] quoted [record] {\"schema\":1}")
            .is_none()
    );
}
