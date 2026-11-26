use super::*;

#[test]
fn check_compacts_only_consecutive_single_line_rows() {
    let should_separate = |command, verbose, previous_has_details, current_has_details| {
        let (mut log, _tmp, _guard) = crate::infra::logging::isolated_logger_for(command);
        log.set_verbose(verbose);
        log.end_task_block(previous_has_details);
        log.should_separate_task_blocks(current_has_details)
    };

    assert!(!should_separate("check", false, false, false));
    for (previous_has_details, current_has_details) in [(true, false), (false, true)] {
        assert!(should_separate(
            "check",
            false,
            previous_has_details,
            current_has_details,
        ));
    }
    assert!(!should_separate("check", true, false, false));
    assert!(!should_separate("install", false, false, false));
}

#[test]
fn changed_task_line_uses_symbol_status() {
    let task = task_entry(
        "symlinks",
        TaskStatus::Changed,
        Some("3 changed, 8 already ok"),
    )
    .with_summary_message(true);

    assert_eq!(
        format_task_line(&task, colored_opts()),
        "\x1b[32m✓\x1b[0m \x1b[1msymlinks\x1b[0m"
    );
    assert_eq!(format_task_line(&task, plain_opts()), "✓ symlinks");
}

#[test]
fn summary_row_classification_is_independent_of_message_wording() {
    for message in ["3 changed, 8 already ok", "aggregate counts"] {
        let task = task_entry("task", TaskStatus::Changed, Some(message));
        assert_eq!(
            format_task_line(&task, plain_opts()),
            format!("✓ task · {message}")
        );
        assert_eq!(
            format_task_line(&task.with_summary_message(true), plain_opts()),
            "✓ task"
        );
    }
}

#[test]
fn task_line_states_reason_beside_task_name() {
    let task = task_entry(
        "Dotfiles repository",
        TaskStatus::Skipped,
        Some("local changes present"),
    );

    assert_eq!(
        format_task_line(&task, plain_opts()),
        "⊘ Dotfiles repository \u{b7} skipped \u{b7} local changes present"
    );
}

#[test]
fn restart_notice_supersedes_changed_task_row() {
    let task = task_entry("Dotfiles repository", TaskStatus::Changed, None)
        .with_result_display(crate::infra::logging::TaskResultDisplay::RestartNotice);

    assert_hidden_row(&task);
}

#[test]
fn restart_notice_tasks_still_report_non_changed_outcomes() {
    let task = task_entry(
        "Dotfiles repository",
        TaskStatus::Skipped,
        Some("local changes present"),
    )
    .with_result_display(crate::infra::logging::TaskResultDisplay::RestartNotice);

    assert_eq!(
        task_result_lines(&task, &[], plain_opts()),
        ["⊘ Dotfiles repository \u{b7} skipped \u{b7} local changes present"]
    );
}

#[test]
fn verbose_task_line_reports_elapsed_time() {
    let mut task = task_entry("Home symlinks", TaskStatus::Ok, None);
    task.duration = Some(Duration::from_millis(1500));
    let opts = RowOpts {
        verbose: true,
        ..plain_opts()
    };

    assert_eq!(format_task_line(&task, opts), "○ Home symlinks \u{b7} 1.5s");
}

#[test]
fn non_verbose_task_line_reports_elapsed_time_only_for_slow_tasks() {
    let mut fast = task_entry("Home symlinks", TaskStatus::Changed, None);
    fast.duration = Some(Duration::from_millis(1999));
    assert_eq!(format_task_line(&fast, plain_opts()), "✓ Home symlinks");

    let mut slow = task_entry("System packages", TaskStatus::Changed, None);
    slow.duration = Some(Duration::from_secs(2));
    assert_eq!(
        format_task_line(&slow, plain_opts()),
        "✓ System packages \u{b7} 2.0s"
    );

    let mut not_applicable = task_entry("Windows registry", TaskStatus::NotApplicable, None);
    not_applicable.duration = Some(Duration::from_secs(5));
    assert_eq!(
        format_task_line(&not_applicable, plain_opts()),
        "⁃ Windows registry"
    );
}

#[test]
fn task_block_drops_the_recorded_summary_not_summary_shaped_text() {
    let task = task_entry(
        "symlinks",
        TaskStatus::Changed,
        Some("2 changed, 1 already ok"),
    )
    .with_summary_message(true);
    let details = vec![
        "link ~/.bashrc".to_string(),
        "2 changed, 1 already ok".to_string(),
        "9 changed, 7 already ok".to_string(),
    ];

    assert_eq!(
        task_result_lines(&task, &details, plain_opts()),
        [
            "✓ symlinks",
            "  link ~/.bashrc",
            "  9 changed, 7 already ok"
        ]
    );
}

#[test]
fn task_block_drops_lines_restating_the_row_reason() {
    let task = task_entry("skip-task", TaskStatus::Skipped, Some("dependency failed"));
    let details = vec![
        "skipped: dependency failed".to_string(),
        "dependency failed".to_string(),
    ];

    assert_eq!(
        task_result_lines(&task, &details, plain_opts()),
        ["⊘ skip-task · skipped · dependency failed"]
    );
}

#[test]
fn task_block_details_are_empty_when_the_task_only_has_a_message() {
    let task = task_entry(
        "custom task",
        TaskStatus::Changed,
        Some("generated private config"),
    );

    assert!(task_block(&task, &[], plain_opts()).details.is_empty());
}

#[test]
fn task_result_lines_are_flat_with_reduced_indent() {
    let task = task_entry("changed-task", TaskStatus::Changed, None);
    let details = vec!["link ~/.example".to_string()];

    assert_eq!(
        task_result_lines(&task, &details, colored_opts()),
        vec![
            "\x1b[32m✓\x1b[0m \x1b[1mchanged-task\x1b[0m",
            "  link ~/.example"
        ]
    );
}

#[test]
fn task_result_lines_omit_success_reason_when_actions_are_listed() {
    let task = task_entry(
        "Agent packages",
        TaskStatus::Changed,
        Some("updated 2 agent packages"),
    );
    let details = vec!["update cursor/plugins/pstack/skills/unslop".to_string()];

    assert_eq!(
        task_result_lines(&task, &details, plain_opts()),
        [
            "✓ Agent packages",
            "  update cursor/plugins/pstack/skills/unslop"
        ]
    );
}

#[test]
fn task_result_lines_preserve_planned_symlink_actions() {
    let mut task = task_entry("Install symlinks", TaskStatus::DryRun, None);
    task.actions = ActionCounts {
        planned: 1,
        ..ActionCounts::default()
    };
    let details = vec!["link ~/.bashrc \u{2192} symlinks/bashrc".to_string()];

    assert_eq!(
        task_result_lines(&task, &details, plain_opts()),
        [
            "~ Install symlinks · planned",
            "  link ~/.bashrc \u{2192} symlinks/bashrc"
        ]
    );
}

#[test]
fn task_result_lines_include_all_details() {
    let task = task_entry("large-plan", TaskStatus::DryRun, None);
    let details = vec![
        (1..=11)
            .map(|index| format!("item {index}"))
            .collect::<Vec<String>>()
            .join("\n"),
    ];

    let lines = task_result_lines(&task, &details, plain_opts());

    assert_eq!(lines.len(), 12);
    assert_eq!(
        lines.first().expect("task status line should exist"),
        "~ large-plan · planned"
    );
    assert_eq!(
        lines.last().expect("last detail line should exist"),
        "  item 11"
    );
}

#[test]
fn task_row_golden_matrix() {
    for (status, glyph, word, color) in [
        (TaskStatus::Changed, "✓", "CHANGED", "32"),
        (TaskStatus::Passed, "✓", "PASSED", "32"),
        (TaskStatus::DryRun, "~", "PLANNED", "35"),
        (TaskStatus::Skipped, "⊘", "SKIPPED", "33"),
        (TaskStatus::Failed, "✗", "FAILED", "31"),
        (TaskStatus::Blocked, "⊖", "BLOCKED", "33"),
        (TaskStatus::Interrupted, "⊗", "INTERRUPTED", "33"),
        (TaskStatus::Ok, "○", "UNCHANGED", "2"),
        (TaskStatus::NotApplicable, "⁃", "N/A", "2"),
    ] {
        let mut task = task_entry("task", status, Some("reason"));
        task.duration = Some(Duration::from_millis(1500));
        for mode in [SummaryMode::Standard, SummaryMode::Check] {
            for symbols in [false, true] {
                for verbose in [false, true] {
                    for ansi in [false, true] {
                        let label = if symbols {
                            glyph
                        } else if status == TaskStatus::Changed && mode == SummaryMode::Check {
                            "PASSED"
                        } else {
                            word
                        };
                        let mut row = if ansi && status == TaskStatus::Ok {
                            format!("\x1b[{color}m{label}\x1b[0m task")
                        } else if ansi {
                            format!("\x1b[{color}m{label}\x1b[0m \x1b[1mtask\x1b[0m")
                        } else {
                            format!("{label} task")
                        };
                        let annotation = match status {
                            TaskStatus::DryRun => Some("planned"),
                            TaskStatus::Skipped => Some("skipped"),
                            TaskStatus::Blocked => Some("blocked"),
                            TaskStatus::Interrupted => Some("interrupted"),
                            TaskStatus::Changed
                            | TaskStatus::Ok
                            | TaskStatus::Passed
                            | TaskStatus::NotApplicable
                            | TaskStatus::Failed => None,
                        };
                        if symbols && let Some(state) = annotation {
                            if ansi {
                                write!(row, "\x1b[2m · \x1b[0m\x1b[{color}m{state}\x1b[0m")
                                    .unwrap();
                            } else {
                                write!(row, " · {state}").unwrap();
                            }
                        }
                        row.push_str(if !ansi {
                            " · reason"
                        } else if matches!(
                            status,
                            TaskStatus::Failed
                                | TaskStatus::Skipped
                                | TaskStatus::Blocked
                                | TaskStatus::Interrupted
                        ) {
                            "\x1b[2m · \x1b[0mreason"
                        } else {
                            "\x1b[2m · reason\x1b[0m"
                        });
                        if verbose && status != TaskStatus::NotApplicable {
                            row.push_str(if ansi {
                                "\x1b[2m · 1.5s\x1b[0m"
                            } else {
                                " · 1.5s"
                            });
                        }
                        let visible = status != TaskStatus::NotApplicable
                            && (verbose || status != TaskStatus::Ok);
                        let expected: Vec<_> = visible.then_some(row).into_iter().collect();
                        assert_eq!(
                            task_result_lines(
                                &task,
                                &[],
                                RowOpts {
                                    mode,
                                    symbols,
                                    columns: None,
                                    verbose,
                                    quiet: false,
                                    style: StyleChoice::auto(ansi, false),
                                    diagnostic_style: StyleChoice::auto(ansi, false),
                                }
                            ),
                            expected,
                            "{status:?}, {mode:?}, symbols={symbols}, verbose={verbose}, ansi={ansi}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn internal_tasks_never_produce_console_rows() {
    let mut task = task_entry(
        "Reload configuration",
        TaskStatus::Skipped,
        Some("dependency failed"),
    );
    task.visibility = TaskVisibility::Internal;

    assert_eq!(
        SummaryCounts::from_tasks(std::slice::from_ref(&task)),
        SummaryCounts::default()
    );
    assert_hidden_row(&task);
}
