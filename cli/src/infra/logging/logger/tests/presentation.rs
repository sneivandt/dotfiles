use super::*;

#[test]
fn separate_after_transition_inserts_blank_line_before_following_output() {
    let log = Logger::build("update", None, Instant::now(), false);
    log.startup("Update · fixture");
    log.transition("CLI upgraded · old → new");
    log.separate_after_transition();
    log.always("task result");
    assert_eq!(
        log.captured_lines(),
        vec![
            "Update · fixture",
            "",
            "CLI upgraded · old → new",
            "",
            "task result",
        ]
    );
}

#[test]
fn foreground_blocks_have_one_separator_and_respect_output_mode() {
    for mode in [
        super::super::super::OutputMode::Human,
        super::super::super::OutputMode::Quiet,
        super::super::super::OutputMode::Jsonl,
    ] {
        let mut log = Logger::build("update", None, Instant::now(), false);
        log.set_output_mode(mode);
        log.startup("Update · fixture");
        log.separate_from_startup();
        log.separate_console_block();
        log.sudo_required("sudo required · System configuration files");
        log.separate_console_block();
        log.separate_console_block();
        log.always("task result");

        let expected = if mode == super::super::super::OutputMode::Human {
            vec![
                "Update · fixture",
                "",
                "⊙ sudo required · System configuration files",
                "",
                "task result",
            ]
        } else {
            vec![]
        };
        assert_eq!(log.captured_lines(), expected, "{mode:?}");
    }
}

#[test]
fn upgrade_follows_the_header_and_groups_with_repository_transition() {
    for verbose in [false, true] {
        for symbols in [false, true] {
            let (mut log, _tmp, _guard) = isolated_logger();
            log.set_verbose(verbose);
            log.set_symbols(symbols);
            log.queue_startup_transition("CLI upgraded · old → new".into());
            assert!(log.captured_lines().is_empty());
            log.startup("Update · desktop profile · Arch Linux");
            log.transition("Repository synced · restarting to load configuration");
            log.separate_from_startup();
            log.separate_console_block();
            log.always("task result");
            assert_eq!(
                log.captured_lines(),
                [
                    "Update · desktop profile · Arch Linux",
                    "",
                    "CLI upgraded · old → new",
                    "Repository synced · restarting to load configuration",
                    "",
                    "task result"
                ],
                "verbose={verbose}, symbols={symbols}"
            );
        }
    }
}

#[test]
fn startup_failure_retains_the_upgrade_notice_once_in_every_output_mode() {
    for mode in [
        super::super::super::OutputMode::Human,
        super::super::super::OutputMode::Quiet,
        super::super::super::OutputMode::Jsonl,
    ] {
        let (mut log, _tmp, _guard) = isolated_logger();
        log.set_output_mode(mode);
        log.queue_startup_transition("CLI upgraded · old → new".into());
        assert_eq!(log.finish_run(1), 1);
        assert_eq!(log.finish_run(1), 1);
        let stored = fs::read_to_string(log.log_path().unwrap()).unwrap();
        assert_eq!(stored.matches("[info] CLI upgraded · old → new").count(), 1);
        let lines = log.captured_lines();
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("CLI upgraded"))
                .count(),
            usize::from(mode == super::super::super::OutputMode::Human),
            "{mode:?}: {lines:?}"
        );
    }
}

#[test]
fn summary_separates_lifecycle_notices_and_includes_prior_process_time() {
    let (mut log, _tmp, _guard) = super::super::super::isolated_logger_for("update");
    log.set_verbose(false);
    log.set_invocation_start(log.invocation_start() - Duration::from_secs(60))
        .unwrap();
    log.startup("Update · fixture");
    log.transition("CLI upgraded · old → new");
    log.print_summary(super::super::super::CompletionDisposition::Succeeded);
    let lines = log.captured_lines();
    assert_eq!(
        &lines[..4],
        ["Update · fixture", "", "CLI upgraded · old → new", "",]
    );
    assert!(
        lines[4].starts_with("No applicable tasks · 1m "),
        "{lines:?}"
    );
}

#[test]
fn task_output_and_transition_use_one_separator_between_blocks() {
    let (log, _tmp, _guard) = isolated_logger();
    log.startup("Update · fixture");
    log.separate_from_startup();
    log.always("task result");
    log.transition("Repository synced · restarting");
    log.separate_console_block();
    log.always("next output");
    assert_eq!(
        log.captured_lines(),
        [
            "Update · fixture",
            "",
            "task result",
            "",
            "Repository synced · restarting",
            "",
            "next output",
        ]
    );
}

#[test]
fn transition_and_recovery_intents_respect_output_mode_and_persist_once() {
    for mode in [
        super::super::super::OutputMode::Human,
        super::super::super::OutputMode::Quiet,
        super::super::super::OutputMode::Jsonl,
    ] {
        let (mut log, _tmp, _guard) = isolated_logger();
        log.set_verbose(false);
        log.set_output_mode(mode);
        log.transition("Repository synced · restarting");
        log.recovery_hint("dotfiles log --id fixture -v");
        let expected: Vec<(bool, &str)> = match mode {
            super::super::super::OutputMode::Human => vec![
                (false, "Repository synced · restarting\n"),
                (false, "  Run 'dotfiles log --id fixture -v' for details.\n"),
            ],
            super::super::super::OutputMode::Quiet => {
                vec![(true, "  Run 'dotfiles log --id fixture -v' for details.\n")]
            }
            super::super::super::OutputMode::Jsonl => vec![],
        };
        let captured: Vec<(bool, String)> = log.captured_output();
        let actual: Vec<(bool, &str)> = captured
            .iter()
            .map(|(err, line)| (*err, line.as_str()))
            .collect();
        assert_eq!(actual, expected, "{mode:?}");
        let stored = fs::read_to_string(log.log_path().unwrap()).unwrap();
        assert_eq!(
            stored
                .matches("[info] Repository synced · restarting")
                .count(),
            1
        );
        assert_eq!(
            stored
                .matches("[info] Run 'dotfiles log --id fixture -v' for details.")
                .count(),
            1
        );
        assert!(!stored.contains('\x1b'));
    }
}

#[test]
fn quiet_mode_emits_failure_and_warning_rows_without_double_prefixes_and_keeps_error_details() {
    use crate::infra::logging::buffered::entry::LogEntry;
    use crate::infra::logging::types::{
        ActionCounts, MsgKind, TaskEntry, TaskStatus, TaskVisibility,
    };

    for (verbose, expected_error_detail) in [
        (false, "  pacman exited with status 1\n"),
        (true, "  error pacman exited with status 1\n"),
    ] {
        let (mut log, _tmp, _guard) = isolated_logger();
        log.set_verbose(verbose);
        log.set_output_mode(super::super::super::OutputMode::Quiet);
        log.record_task(TaskEntry::new(
            "ok-task",
            "Home symlinks",
            TaskStatus::Changed,
            None,
            ActionCounts::default(),
            TaskVisibility::Visible,
        ));
        log.emit_recorded_task_result("ok-task", &[]);

        log.record_task(TaskEntry::new(
            "skip-task",
            "Dotfiles repository",
            TaskStatus::Skipped,
            Some("local changes present"),
            ActionCounts::default(),
            TaskVisibility::Visible,
        ));
        log.emit_recorded_task_result("skip-task", &[]);

        log.record_task(TaskEntry::new(
            "fail-task",
            "System packages",
            TaskStatus::Failed,
            None,
            ActionCounts::default(),
            TaskVisibility::Visible,
        ));
        log.emit_recorded_task_result(
            "fail-task",
            &[LogEntry::Message {
                kind: MsgKind::Error,
                msg: "pacman exited with status 1".into(),
            }],
        );

        let captured = log.captured_output();
        assert!(
            captured.iter().all(|(err, _)| *err),
            "quiet mode must never write to stdout (verbose={verbose}): {captured:?}"
        );
        let lines: Vec<&str> = captured.iter().map(|(_, line)| line.as_str()).collect();
        assert_eq!(
            lines,
            [
                "⊘ Dotfiles repository · skipped · local changes present\n",
                "✗ System packages\n",
                expected_error_detail,
            ],
            "verbose={verbose}"
        );
    }
}

#[test]
fn summary_separates_startup_context_before_totals() {
    let (log, _tmp, _guard) = isolated_logger();
    log.startup("Install · desktop profile · Arch Linux");

    log.print_summary(super::super::super::CompletionDisposition::Succeeded);

    assert!(
        log.console.lock().startup_separator_emitted,
        "a run without visible task rows still needs one separator before totals"
    );
}

#[test]
fn command_titles_are_sentence_case() {
    let cache = tempfile::tempdir().expect("temporary cache should be created");
    for (command, expected) in [
        ("install", "Install"),
        ("uninstall", "Uninstall"),
        ("check", "Check"),
    ] {
        let log = Logger::new_in(command, cache.path());
        assert_eq!(log.command_title(), expected);
    }
}

#[test]
fn public_status_operations_draw_only_on_a_terminal_and_clear_the_row() {
    for terminal in [false, true] {
        let mut log = Logger::test();
        log.progress_terminal = terminal;
        log.status_line("Checking");
        assert_eq!(log.progress_rows_count(), u16::from(terminal));
        assert_eq!(
            log.captured_lines()
                .iter()
                .any(|line| line.contains("Checking")),
            terminal
        );
        log.clear_status_line();
        assert_eq!(log.progress_rows_count(), 0);
        assert!(log.console.lock().custom_progress.is_none());
    }
}

#[test]
fn dry_run_uses_the_preview_progress_color_until_cleared() {
    let mut log = Logger::test();
    Arc::get_mut(&mut log.console).unwrap().stdout_style =
        crate::infra::logging::style::StyleChoice::colored();
    for (dry_run, color) in [(true, "\x1b[35m"), (false, "\x1b[36m")] {
        log.set_dry_run(dry_run);
        log.replace_status_line("Checking");
        assert!(log.captured_output().last().unwrap().1.contains(color));
    }
}
