use std::fmt::Write as _;
use std::time::Duration;

use super::render::{RowOpts, format_task_line, task_block};
use super::totals::{SummaryCounts, SummaryMode, should_space_before_totals};
use crate::infra::logging::CompletionDisposition;
use crate::infra::logging::buffered::entry::LogEntry;
use crate::infra::logging::style::StyleChoice;
use crate::infra::logging::types::{ActionCounts, MsgKind, TaskEntry, TaskStatus, TaskVisibility};
use crate::infra::logging::utils::format_elapsed;
use crate::infra::logging::{Logger, OutputExt as _};

/// Build a task entry with the fields a row-rendering test cares about.
fn task_entry(name: &str, status: TaskStatus, message: Option<&str>) -> TaskEntry {
    TaskEntry::new(
        name,
        name,
        status,
        message,
        ActionCounts::default(),
        TaskVisibility::Visible,
    )
}

fn record_task(log: &Logger, name: &str, status: TaskStatus, message: Option<&str>) {
    log.record_task(task_entry(name, status, message));
}

fn task_result_lines(task: &TaskEntry, details: &[String], opts: RowOpts) -> Vec<String> {
    let entries: Vec<_> = details
        .iter()
        .map(|message| LogEntry::Message {
            kind: MsgKind::Info,
            msg: message.clone(),
        })
        .collect();
    let block = task_block(task, &entries, opts);
    block
        .status
        .into_iter()
        .chain(
            block
                .details
                .into_iter()
                .flat_map(|detail| detail.line.lines().map(str::to_string).collect::<Vec<_>>()),
        )
        .collect()
}

/// Standard-mode, non-verbose row options.
fn plain_opts() -> RowOpts {
    RowOpts {
        mode: SummaryMode::Standard,
        style: StyleChoice::plain(),
        diagnostic_style: StyleChoice::plain(),
        symbols: true,
        columns: None,
        verbose: false,
    }
}

/// Standard-mode, non-verbose row options with colour enabled.
fn colored_opts() -> RowOpts {
    RowOpts {
        style: StyleChoice::colored(),
        diagnostic_style: StyleChoice::colored(),
        ..plain_opts()
    }
}

/// Count/vocabulary fixtures use successful completion; disposition is tested separately.
fn format_summary_lines(
    counts: SummaryCounts,
    mode: SummaryMode,
    elapsed: &str,
    style: StyleChoice,
) -> Vec<String> {
    super::totals::format_summary_lines(
        counts,
        mode,
        elapsed,
        style,
        CompletionDisposition::Succeeded,
    )
}

#[test]
fn format_elapsed_values() {
    assert_eq!(format_elapsed(Duration::from_millis(450)), "0.5s");
    assert_eq!(format_elapsed(Duration::from_secs_f64(3.7)), "3.7s");
    assert_eq!(format_elapsed(Duration::from_secs(125)), "2m 5s");
}

#[test]
fn standard_empty_summary_limits_its_claim_to_tasks() {
    let plain_lines = format_summary_lines(
        SummaryCounts::default(),
        SummaryMode::Standard,
        "1.2s",
        StyleChoice::plain(),
    );
    let colored_lines = format_summary_lines(
        SummaryCounts::default(),
        SummaryMode::Standard,
        "1.2s",
        StyleChoice::colored(),
    );

    assert_eq!(plain_lines, ["No applicable tasks · 1.2s"]);
    assert_eq!(
        colored_lines,
        ["\x1b[1mNo applicable tasks\x1b[0m \x1b[2m·\x1b[0m \x1b[2m1.2s\x1b[0m"]
    );
}

#[test]
fn current_only_summary_is_bold_without_a_contradictory_no_changes_label() {
    for (count, text) in [(1, "1 unchanged"), (17, "17 unchanged")] {
        for ansi in [false, true] {
            let style = StyleChoice::auto(ansi, false);
            assert_eq!(
                format_summary_lines(
                    SummaryCounts {
                        ok: count,
                        ..SummaryCounts::default()
                    },
                    SummaryMode::Standard,
                    "3.4s",
                    style,
                ),
                [format!(
                    "{} {} {} {}",
                    style.paint(crate::infra::logging::style::TextStyle::Dim, "Tasks:"),
                    style.paint(crate::infra::logging::style::TextStyle::Bold, text),
                    style.paint(crate::infra::logging::style::TextStyle::Dim, "·"),
                    style.paint(crate::infra::logging::style::TextStyle::Dim, "3.4s"),
                )]
            );
        }
    }
}

#[test]
fn interrupted_partial_work_is_not_reported_as_no_changes() {
    for dry_run in [false, true] {
        let mut task = task_entry(
            "partial batch",
            TaskStatus::Interrupted,
            Some("interrupted"),
        );
        task.duration = Some(Duration::from_secs(1));
        task.actions = ActionCounts {
            applied: u32::from(!dry_run),
            planned: u32::from(dry_run),
            not_attempted: 2,
            ..ActionCounts::default()
        };
        let counts = SummaryCounts::from_tasks(&[task]);
        assert_eq!(counts.actions.not_attempted, 2);
        assert_eq!(
            format_summary_lines(counts, SummaryMode::Standard, "1.0s", StyleChoice::plain(),),
            ["Tasks: 1 interrupted · 1.0s"],
        );
    }
}

#[test]
fn standard_summary_totals_preserve_task_and_action_counts() {
    for (case, counts, elapsed, expected) in [
        (
            "standard_error_summary_starts_with_failed_count",
            SummaryCounts {
                ok: 1,
                failed: 1,
                ..SummaryCounts::default()
            },
            "7.2s",
            "Tasks: 1 failed · 1 unchanged · 7.2s",
        ),
        (
            "standard_summary_groups_task_and_action_counts",
            SummaryCounts {
                changed: 3,
                skipped: 1,
                failed: 1,
                actions: ActionCounts {
                    applied: 87,
                    skipped: 2,
                    failed: 1,
                    ..ActionCounts::default()
                },
                ..SummaryCounts::default()
            },
            "2.0s",
            "Tasks: 1 failed · 1 skipped · 3 changed · 2.0s",
        ),
        (
            "dry_run_summary_pairs_affected_and_planned_counts",
            SummaryCounts {
                dry_run: 1,
                actions: ActionCounts {
                    planned: 81,
                    ..ActionCounts::default()
                },
                ..SummaryCounts::default()
            },
            "0.8s",
            "Tasks: 1 planned · 0.8s",
        ),
        (
            "dry_run_summary_counts_unquantified_affected_tasks",
            SummaryCounts {
                dry_run: 2,
                ..SummaryCounts::default()
            },
            "0.8s",
            "Tasks: 2 planned · 0.8s",
        ),
        (
            "summary_totals_account_for_every_reported_task",
            SummaryCounts {
                changed: 2,
                ok: 15,
                skipped: 1,
                actions: ActionCounts {
                    applied: 4,
                    ..ActionCounts::default()
                },
                ..SummaryCounts::default()
            },
            "2.3s",
            "Tasks: 1 skipped · 2 changed · 15 unchanged · 2.3s",
        ),
        (
            "standard_summary_omits_actions_when_all_action_counts_are_zero",
            SummaryCounts {
                changed: 2,
                ..SummaryCounts::default()
            },
            "1.0s",
            "Tasks: 2 changed · 1.0s",
        ),
    ] {
        assert_eq!(
            format_summary_lines(counts, SummaryMode::Standard, elapsed, StyleChoice::plain()),
            [expected],
            "{case}",
        );
    }
}

#[test]
fn check_summary_uses_check_vocabulary_and_omits_not_run() {
    let counts = SummaryCounts {
        passed: 7,
        skipped: 2,
        failed: 1,
        ..SummaryCounts::default()
    };
    assert_eq!(
        format_summary_lines(counts, SummaryMode::Check, "3.4s", StyleChoice::plain()),
        ["Checks: 1 failed · 2 skipped · 7 passed · 3.4s"],
    );
}

#[test]
fn no_op_mutation_commands_skip_extra_blank() {
    for command in ["install", "update", "uninstall", "remove"] {
        assert!(
            !should_space_before_totals(command, false),
            "{command} no-op runs should not add an extra separator"
        );
    }
    assert!(should_space_before_totals("update", true));
    assert!(should_space_before_totals("check", false));
}

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

    assert!(task_result_lines(&task, &[], plain_opts()).is_empty());
    assert!(
        task_result_lines(
            &task,
            &[],
            RowOpts {
                verbose: true,
                ..plain_opts()
            }
        )
        .is_empty()
    );
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
        (TaskStatus::Blocked, "⊘", "BLOCKED", "33"),
        (TaskStatus::Interrupted, "⊘", "INTERRUPTED", "33"),
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
    assert!(task_result_lines(&task, &[], plain_opts()).is_empty());
    assert!(
        task_result_lines(
            &task,
            &[],
            RowOpts {
                verbose: true,
                ..plain_opts()
            }
        )
        .is_empty()
    );
}

#[test]
fn unstarted_interruptions_do_not_produce_rows_or_totals() {
    let queued = task_entry("queued", TaskStatus::Interrupted, Some("cancelled"));
    let mut started = task_entry("started", TaskStatus::Interrupted, Some("interrupted"));
    started.duration = Some(Duration::from_millis(100));

    assert!(task_result_lines(&queued, &[], plain_opts()).is_empty());
    assert!(
        task_result_lines(
            &queued,
            &[],
            RowOpts {
                verbose: true,
                ..plain_opts()
            }
        )
        .is_empty()
    );
    assert_eq!(
        task_result_lines(&started, &[], plain_opts()),
        ["⊘ started · interrupted"]
    );

    let counts = SummaryCounts::from_tasks(&[queued, started]);
    assert_eq!(counts.interrupted, 1);
}

#[test]
fn colored_summary_styles_each_outcome_group() {
    let lines = format_summary_lines(
        SummaryCounts {
            changed: 1,
            ok: 2,
            skipped: 3,
            failed: 4,
            actions: ActionCounts {
                applied: 2,
                ..ActionCounts::default()
            },
            ..SummaryCounts::default()
        },
        SummaryMode::Standard,
        "1.0s",
        StyleChoice::colored(),
    );

    assert_eq!(
        lines,
        ["\x1b[2mTasks:\x1b[0m \x1b[1;31m4 failed\x1b[0m \
             \x1b[2m·\x1b[0m \x1b[1;33m3 skipped\x1b[0m \
             \x1b[2m·\x1b[0m \x1b[32m1 changed\x1b[0m \
             \x1b[2m·\x1b[0m \x1b[2m2 unchanged\x1b[0m \
             \x1b[2m·\x1b[0m \x1b[2m1.0s\x1b[0m"]
    );
}

#[test]
fn print_summary_clears_visible_progress() {
    let (log, _tmp, _guard) = crate::infra::logging::isolated_logger();
    record_task(&log, "changed-task", TaskStatus::Changed, None);
    log.notify_task_start_with_progress("active-task", true);

    assert!(log.progress_rows_count() > 0);
    assert!(log.status_row_visible());

    log.print_summary(CompletionDisposition::Succeeded);

    assert_eq!(log.progress_rows_count(), 0);
    assert!(!log.status_row_visible());
}

#[test]
fn no_op_install_summary_needs_no_totals_separator() {
    let (mut log, _tmp, _guard) = crate::infra::logging::isolated_logger_for("install");
    log.set_verbose(false);
    for index in 0..3 {
        record_task(&log, &format!("task-{index}"), TaskStatus::Ok, None);
    }
    assert!(
        !log.needs_totals_separator(),
        "runs that printed nothing should not emit a blank line before the totals"
    );
}

#[test]
fn no_op_update_summary_reuses_the_startup_separator() {
    let (log, _tmp, _guard) = crate::infra::logging::isolated_logger_for("update");
    log.startup("Update · desktop profile · Arch Linux");
    log.separate_from_startup();

    assert!(log.console_ends_with_blank_line());
    assert!(
        !log.needs_totals_separator(),
        "the startup separator should be the only blank line before no-op totals"
    );
}

#[test]
fn install_summary_needs_totals_separator_after_task_output() {
    let (mut log, _tmp, _guard) = crate::infra::logging::isolated_logger_for("install");
    log.set_verbose(false);
    record_task(&log, "task-changed", TaskStatus::Changed, None);
    log.mark_task_console_output();

    assert!(log.needs_totals_separator());
}

#[test]
fn blocked_and_interrupted_counts_do_not_become_skips_or_failures() {
    let mut tasks = [
        task_entry(
            "skip",
            TaskStatus::Skipped,
            Some("optional tool unavailable"),
        ),
        task_entry(
            "blocked",
            TaskStatus::Blocked,
            Some("requires prerequisite"),
        ),
        task_entry("interrupted", TaskStatus::Interrupted, Some("interrupted")),
    ];
    tasks[2].duration = Some(Duration::from_millis(100));
    let counts = SummaryCounts::from_tasks(&tasks);
    assert_eq!(
        (
            counts.skipped,
            counts.blocked,
            counts.interrupted,
            counts.failed
        ),
        (1, 1, 1, 0)
    );
    assert_eq!(
        format_summary_lines(counts, SummaryMode::Check, "1.0s", StyleChoice::plain()),
        ["Checks: 1 interrupted · 1 blocked · 1 skipped · 1.0s"]
    );
}

#[test]
fn incomplete_work_never_claims_no_changes() {
    for counts in [
        SummaryCounts {
            skipped: 1,
            ..SummaryCounts::default()
        },
        SummaryCounts {
            blocked: 1,
            ..SummaryCounts::default()
        },
        SummaryCounts {
            interrupted: 1,
            ..SummaryCounts::default()
        },
        SummaryCounts {
            failed: 1,
            ..SummaryCounts::default()
        },
    ] {
        let lines =
            format_summary_lines(counts, SummaryMode::Standard, "1.0s", StyleChoice::plain());
        assert!(!lines[0].contains("No applicable tasks"), "{lines:?}");
        assert!(!lines[0].contains("unchanged"), "{lines:?}");
    }
}

#[test]
fn all_incomplete_totals_are_bold_and_precede_successes() {
    let lines = format_summary_lines(
        SummaryCounts {
            failed: 1,
            interrupted: 2,
            blocked: 3,
            skipped: 4,
            changed: 5,
            dry_run: 6,
            ok: 7,
            ..SummaryCounts::default()
        },
        SummaryMode::Standard,
        "1.0s",
        StyleChoice::colored(),
    );
    assert_eq!(
        lines,
        [
            "\x1b[2mTasks:\x1b[0m \x1b[1;31m1 failed\x1b[0m \x1b[2m·\x1b[0m \x1b[1;33m2 interrupted\x1b[0m \x1b[2m·\x1b[0m \x1b[1;33m3 blocked\x1b[0m \x1b[2m·\x1b[0m \x1b[1;33m4 skipped\x1b[0m \x1b[2m·\x1b[0m \x1b[32m5 changed\x1b[0m \x1b[2m·\x1b[0m \x1b[35m6 planned\x1b[0m \x1b[2m·\x1b[0m \x1b[2m7 unchanged\x1b[0m \x1b[2m·\x1b[0m \x1b[2m1.0s\x1b[0m"
        ]
    );
}

#[test]
fn symbol_rows_name_states_even_without_reasons_or_with_action_details() {
    for (status, state) in [
        (TaskStatus::DryRun, "planned"),
        (TaskStatus::Skipped, "skipped"),
        (TaskStatus::Blocked, "blocked"),
        (TaskStatus::Interrupted, "interrupted"),
    ] {
        let mut task = task_entry("task", status, None);
        task.duration = Some(Duration::from_secs(1));
        for verbose in [false, true] {
            let block = task_block(
                &task,
                &[LogEntry::action(
                    "write",
                    "example",
                    status == TaskStatus::DryRun,
                    "write example",
                )],
                RowOpts {
                    verbose,
                    ..plain_opts()
                },
            );
            let glyph = if status == TaskStatus::DryRun {
                "~"
            } else {
                "⊘"
            };
            let timing = if verbose { " · 1.0s" } else { "" };
            assert_eq!(
                block.status.as_deref(),
                Some(format!("{glyph} task · {state}{timing}").as_str())
            );
            assert_eq!(block.details.len(), 1);
        }
        task.message = Some(state.into());
        assert_eq!(
            format_task_line(&task, plain_opts()).matches(state).count(),
            1
        );
    }
}

#[test]
fn expanded_action_badges_use_structured_counts_in_both_modes() {
    for verbose in [false, true] {
        for (status, count, expected) in [
            (TaskStatus::Changed, 0, "✓ task"),
            (TaskStatus::Changed, 1, "✓ task"),
            (TaskStatus::Changed, 35, "✓ task · 35 actions applied"),
            (TaskStatus::DryRun, 0, "~ task · planned"),
            (TaskStatus::DryRun, 1, "~ task · planned"),
            (TaskStatus::DryRun, 35, "~ task · planned · 35 actions"),
        ] {
            let mut task = task_entry("task", status, None);
            task.actions = ActionCounts {
                applied: if status == TaskStatus::Changed {
                    count
                } else {
                    999
                },
                planned: if status == TaskStatus::DryRun {
                    count
                } else {
                    999
                },
                ..ActionCounts::default()
            };
            let opts = RowOpts {
                verbose,
                ..plain_opts()
            };
            let block = task_block(
                &task,
                &[LogEntry::action(
                    "write",
                    "item",
                    status == TaskStatus::DryRun,
                    "write item",
                )],
                opts,
            );
            assert!(block.expanded);
            assert_eq!(block.status.as_deref(), Some(expected));
            let collapsed = task_block(&task, &[], opts);
            assert!(!collapsed.expanded);
            assert!(!collapsed.status.unwrap().contains("actions"));
        }
    }
}

#[test]
fn hidden_details_timings_and_counts_do_not_expand_verbose_rows() {
    let mut task = task_entry("task", TaskStatus::Ok, None);
    task.duration = Some(Duration::from_secs(1));
    task.actions.applied = 35;
    let entries = [
        MsgKind::Stage,
        MsgKind::Trace,
        MsgKind::Summary,
        MsgKind::Debug,
    ]
    .map(|kind| LogEntry::Message {
        kind,
        msg: if kind == MsgKind::Debug {
            " \t".into()
        } else {
            "hidden".into()
        },
    });
    let block = task_block(
        &task,
        &entries,
        RowOpts {
            verbose: true,
            ..plain_opts()
        },
    );
    assert!(!block.expanded);
    assert_eq!(block.status.as_deref(), Some("○ task · 1.0s"));
}

#[test]
fn final_summary_preserves_disposition_and_emphasis_without_a_symbol_prefix() {
    for mode in [SummaryMode::Standard, SummaryMode::Check] {
        for (disposition, counts, emphasis, text) in [
            (
                CompletionDisposition::Failed,
                SummaryCounts::default(),
                "1;31",
                "Failed",
            ),
            (
                CompletionDisposition::Interrupted,
                SummaryCounts::default(),
                "1;33",
                "Interrupted",
            ),
            (
                CompletionDisposition::Succeeded,
                SummaryCounts {
                    skipped: 1,
                    ..SummaryCounts::default()
                },
                "1;33",
                "1 skipped",
            ),
            (
                CompletionDisposition::Succeeded,
                SummaryCounts::default(),
                "1",
                if mode == SummaryMode::Check {
                    "No checks ran"
                } else {
                    "No applicable tasks"
                },
            ),
        ] {
            for ansi in [false, true] {
                let lines = super::totals::format_summary_lines(
                    counts,
                    mode,
                    "1.0s",
                    StyleChoice::auto(ansi, false),
                    disposition,
                );
                let clean = crate::infra::logging::utils::strip_ansi(&lines[0]);
                let unit = if mode == SummaryMode::Check {
                    "Checks:"
                } else {
                    "Tasks:"
                };
                let prefix = if counts.skipped > 0 {
                    format!("{unit} ")
                } else {
                    String::new()
                };
                assert_eq!(clean, format!("{prefix}{text} · 1.0s"));
                if ansi {
                    let styled_prefix = if counts.skipped > 0 {
                        format!("\x1b[2m{unit}\x1b[0m ")
                    } else {
                        String::new()
                    };
                    assert!(
                        lines[0]
                            .starts_with(&format!("{styled_prefix}\x1b[{emphasis}m{text}\x1b[0m"))
                    );
                }
            }
        }
    }
}

#[test]
fn completed_blocks_preserve_typed_action_style_and_verbose_message_presentation() {
    for verbose in [false, true] {
        let opts = RowOpts {
            verbose,
            diagnostic_style: StyleChoice::plain(),
            ..colored_opts()
        };
        for planned in [false, true] {
            let task = task_entry(
                "task",
                if planned {
                    TaskStatus::DryRun
                } else {
                    TaskStatus::Changed
                },
                None,
            );
            let block = task_block(
                &task,
                &[LogEntry::action(
                    "write",
                    "item → source",
                    planned,
                    "write item → source (extra detail)",
                )],
                opts,
            );
            let color = if planned { "35" } else { "36" };
            assert_eq!(
                block.details[0].line,
                format!("  \x1b[{color}mwrite\x1b[0m  item → source\x1b[2m (extra detail)\x1b[0m"),
                "verbose={verbose}, planned={planned}",
            );
        }
        let block = task_block(
            &task_entry("task", TaskStatus::Changed, None),
            &[LogEntry::Message {
                kind: MsgKind::Always,
                msg: "notice".into(),
            }],
            opts,
        );
        assert_eq!(
            block.details[0].line,
            if verbose { "notice" } else { "  notice" }
        );
    }
}

#[test]
fn completed_actions_align_visible_subjects_without_forcing_wrap() {
    let task = task_entry("task", TaskStatus::Changed, Some("reconfigure hidden"));
    let entries = [
        LogEntry::action("更新", "one → two", false, "更新 one → two (detail)"),
        LogEntry::Message {
            kind: MsgKind::Warn,
            msg: "check permissions".into(),
        },
        LogEntry::action("install", "fd", false, "install fd"),
        LogEntry::action("reconfigure", "hidden", false, "reconfigure hidden"),
        LogEntry::action("synchronize", "item", false, "custom action wording"),
        LogEntry::action("refresh\tcache", "item", false, "refresh\tcache item"),
    ];

    for (columns, first_line) in [
        (None, "  更新     one → two (detail)"),
        (Some(27), "  更新   one → two (detail)"),
        (Some(25), "  更新 one → two (detail)"),
        (Some(24), "  更新     one → two (detail)"),
    ] {
        let block = task_block(
            &task,
            &entries,
            RowOpts {
                columns,
                ..colored_opts()
            },
        );
        let lines: Vec<_> = block
            .details
            .iter()
            .map(|detail| crate::infra::logging::utils::strip_ansi(&detail.line))
            .collect();
        assert_eq!(
            lines,
            [
                first_line,
                "  WARN  check permissions",
                "  install  fd",
                "  custom action wording",
                "  refresh\tcache item",
            ],
            "terminal columns: {columns:?}",
        );
        assert!(block.details[1].error_stream);
    }
}

#[test]
fn incomplete_summaries_report_each_outcome_without_duplicate_disposition_text() {
    for (counts, incomplete_disposition, text) in [
        (
            SummaryCounts {
                failed: 1,
                ..SummaryCounts::default()
            },
            CompletionDisposition::Failed,
            "1 failed",
        ),
        (
            SummaryCounts {
                interrupted: 1,
                ..SummaryCounts::default()
            },
            CompletionDisposition::Interrupted,
            "1 interrupted",
        ),
        (
            SummaryCounts {
                blocked: 1,
                ..SummaryCounts::default()
            },
            CompletionDisposition::Succeeded,
            "1 blocked",
        ),
        (
            SummaryCounts {
                skipped: 1,
                ..SummaryCounts::default()
            },
            CompletionDisposition::Succeeded,
            "1 skipped",
        ),
    ] {
        for mode in [SummaryMode::Standard, SummaryMode::Check] {
            for disposition in [CompletionDisposition::Succeeded, incomplete_disposition] {
                let lines = super::totals::format_summary_lines(
                    counts,
                    mode,
                    "1.0s",
                    StyleChoice::plain(),
                    disposition,
                );
                let unit = if mode == SummaryMode::Check {
                    "Checks"
                } else {
                    "Tasks"
                };
                assert_eq!(
                    lines,
                    [format!("{unit}: {text} · 1.0s")],
                    "{mode:?}, {disposition:?}"
                );
            }
        }
    }
}

#[test]
fn a_warning_does_not_hide_a_changed_tasks_custom_reason_or_use_stdout_styling() {
    let task = task_entry("task", TaskStatus::Changed, Some("custom outcome"));
    let entries = [LogEntry::Message {
        kind: MsgKind::Warn,
        msg: "check permissions".into(),
    }];
    let block = task_block(
        &task,
        &entries,
        RowOpts {
            diagnostic_style: StyleChoice::plain(),
            ..colored_opts()
        },
    );
    assert!(block.status.unwrap().contains("custom outcome"));
    assert!(block.expanded);
    assert_eq!(block.details[0].line, "  WARN  check permissions");
    assert!(block.details[0].error_stream);
}
