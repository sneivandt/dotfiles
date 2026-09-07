use std::time::Duration;

use super::render::{RowOpts, format_task_line, task_detail_lines, task_result_lines};
use super::totals::{SummaryCounts, SummaryMode, format_summary_lines, should_space_before_totals};
use crate::infra::logging::Logger;
use crate::infra::logging::logger::TaskDetailEntry;
use crate::infra::logging::style::StyleChoice;
use crate::infra::logging::types::{ActionCounts, TaskEntry, TaskStatus, TaskVisibility};
use crate::infra::logging::utils::format_elapsed;

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

/// Standard-mode, non-verbose row options.
fn plain_opts() -> RowOpts {
    RowOpts {
        mode: SummaryMode::Standard,
        style: StyleChoice::plain(),
        symbols: true,
        verbose: false,
    }
}

/// Standard-mode, non-verbose row options with colour enabled.
fn colored_opts() -> RowOpts {
    RowOpts {
        style: StyleChoice::colored(),
        ..plain_opts()
    }
}

#[test]
fn format_elapsed_values() {
    assert_eq!(format_elapsed(Duration::from_millis(450)), "0.5s");
    assert_eq!(format_elapsed(Duration::from_secs_f64(3.7)), "3.7s");
    assert_eq!(format_elapsed(Duration::from_secs(125)), "2m 5s");
}

#[test]
fn standard_no_op_has_only_no_changes_line() {
    let plain_lines = format_summary_lines(
        SummaryCounts::default(),
        SummaryMode::Standard,
        false,
        "1.2s",
        StyleChoice::plain(),
    );
    let colored_lines = format_summary_lines(
        SummaryCounts::default(),
        SummaryMode::Standard,
        false,
        "1.2s",
        StyleChoice::colored(),
    );

    assert_eq!(plain_lines, ["No changes · 1.2s"]);
    assert_eq!(
        colored_lines,
        ["\x1b[1mNo changes\x1b[0m \x1b[2m·\x1b[0m \x1b[2m1.2s\x1b[0m"]
    );
}

#[test]
fn standard_error_summary_starts_with_failed_count() {
    let lines = format_summary_lines(
        SummaryCounts {
            ok: 1,
            failed: 1,
            ..SummaryCounts::default()
        },
        SummaryMode::Standard,
        false,
        "7.2s",
        StyleChoice::plain(),
    );

    assert_eq!(lines, ["1 failed · 1 current · 7.2s"]);
}

#[test]
fn standard_summary_groups_task_and_action_counts() {
    let lines = format_summary_lines(
        SummaryCounts {
            changed: 3,
            passed: 0,
            blocked: 0,
            interrupted: 0,
            ok: 0,
            skipped: 1,
            dry_run: 0,
            failed: 1,
            actions: ActionCounts {
                applied: 87,
                planned: 0,
                skipped: 2,
                failed: 1,
            },
        },
        SummaryMode::Standard,
        false,
        "2.0s",
        StyleChoice::plain(),
    );

    assert_eq!(lines, ["1 failed · 3 changed · 1 skipped · 2.0s"]);
}

#[test]
fn dry_run_summary_pairs_affected_and_planned_counts() {
    let lines = format_summary_lines(
        SummaryCounts {
            changed: 0,
            passed: 0,
            blocked: 0,
            interrupted: 0,
            ok: 0,
            skipped: 0,
            dry_run: 1,
            failed: 0,
            actions: ActionCounts {
                planned: 81,
                ..ActionCounts::default()
            },
        },
        SummaryMode::Standard,
        true,
        "0.8s",
        StyleChoice::plain(),
    );

    assert_eq!(lines, ["1 would change · 0.8s"]);
}

#[test]
fn dry_run_summary_counts_unquantified_affected_tasks() {
    let lines = format_summary_lines(
        SummaryCounts {
            dry_run: 2,
            ..SummaryCounts::default()
        },
        SummaryMode::Standard,
        true,
        "0.8s",
        StyleChoice::plain(),
    );

    assert_eq!(lines, ["2 would change · 0.8s"]);
}

#[test]
fn summary_totals_account_for_every_reported_task() {
    let lines = format_summary_lines(
        SummaryCounts {
            changed: 2,
            passed: 0,
            blocked: 0,
            interrupted: 0,
            ok: 15,
            skipped: 1,
            dry_run: 0,
            failed: 0,
            actions: ActionCounts {
                applied: 4,
                ..ActionCounts::default()
            },
        },
        SummaryMode::Standard,
        false,
        "2.3s",
        StyleChoice::plain(),
    );

    assert_eq!(
        lines,
        ["2 changed \u{b7} 15 current \u{b7} 1 skipped \u{b7} 2.3s"],
        "every task the run reported on must be represented in the totals"
    );
}

#[test]
fn standard_summary_omits_actions_when_all_action_counts_are_zero() {
    let lines = format_summary_lines(
        SummaryCounts {
            changed: 2,
            passed: 0,
            blocked: 0,
            interrupted: 0,
            ok: 0,
            skipped: 0,
            dry_run: 0,
            failed: 0,
            actions: ActionCounts::default(),
        },
        SummaryMode::Standard,
        false,
        "1.0s",
        StyleChoice::plain(),
    );

    assert_eq!(lines, ["2 changed · 1.0s"]);
}

#[test]
fn check_summary_uses_check_vocabulary_and_omits_not_run() {
    let lines = format_summary_lines(
        SummaryCounts {
            changed: 0,
            passed: 7,
            blocked: 0,
            interrupted: 0,
            ok: 0,
            skipped: 2,
            dry_run: 0,
            failed: 1,
            actions: ActionCounts::default(),
        },
        SummaryMode::Check,
        false,
        "3.4s",
        StyleChoice::plain(),
    );

    assert_eq!(lines, ["1 failed · 7 passed · 2 skipped · 3.4s"]);
}

#[test]
fn no_op_standard_commands_skip_extra_blank() {
    for command in ["install", "uninstall"] {
        assert!(
            !should_space_before_totals(command, false),
            "{command} no-op runs should not add an extra separator"
        );
    }
    assert!(should_space_before_totals("install", true));
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
    assert!(should_separate("check", true, false, false));
    assert!(should_separate("install", false, false, false));
}

#[test]
fn changed_task_line_uses_symbol_status() {
    let task = task_entry(
        "symlinks",
        TaskStatus::Changed,
        Some("3 changed, 8 already ok"),
    );

    assert_eq!(
        format_task_line(&task, colored_opts()),
        "\x1b[32m✓\x1b[0m \x1b[1msymlinks\x1b[0m"
    );
    assert_eq!(format_task_line(&task, plain_opts()), "✓ symlinks");
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
        "⊘ Dotfiles repository \u{b7} local changes present"
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
fn task_detail_lines_filters_generic_stats_summary() {
    let task = task_entry(
        "symlinks",
        TaskStatus::Changed,
        Some("2 changed, 1 already ok"),
    );
    let details = vec![TaskDetailEntry {
        task_id: "symlinks".to_string(),
        lines: vec![
            "linked: ~/.bashrc".to_string(),
            "2 changed, 1 already ok".to_string(),
        ],
    }];

    assert_eq!(
        task_detail_lines(&details, &task),
        vec!["linked: ~/.bashrc"]
    );
}

#[test]
fn task_detail_lines_drops_lines_restating_the_row_reason() {
    let task = task_entry("skip-task", TaskStatus::Skipped, Some("dependency failed"));
    let details = vec![TaskDetailEntry {
        task_id: "skip-task".to_string(),
        lines: vec![
            "skipped: dependency failed".to_string(),
            "dependency failed".to_string(),
        ],
    }];

    assert!(task_detail_lines(&details, &task).is_empty());
}

#[test]
fn task_detail_lines_are_empty_when_the_task_only_has_a_message() {
    let task = task_entry(
        "custom task",
        TaskStatus::Changed,
        Some("generated private config"),
    );

    assert!(task_detail_lines(&[], &task).is_empty());
}

#[test]
fn task_result_lines_are_flat_with_reduced_indent() {
    let task = task_entry("changed-task", TaskStatus::Changed, None);
    let details = vec![TaskDetailEntry {
        task_id: "changed-task".to_string(),
        lines: vec!["linked: ~/.example".to_string()],
    }];

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
        "APM package updates",
        TaskStatus::Changed,
        Some("updated 2 APM dependencies"),
    );
    let details = vec![TaskDetailEntry {
        task_id: task.task_id.clone(),
        lines: vec!["updated: cursor/plugins/pstack/skills/unslop".to_string()],
    }];

    assert_eq!(
        task_result_lines(&task, &details, plain_opts()),
        [
            "✓ APM package updates",
            "  update cursor/plugins/pstack/skills/unslop"
        ]
    );
}

#[test]
fn task_result_lines_abbreviate_symlink_actions() {
    let mut task = task_entry("Install symlinks", TaskStatus::DryRun, None);
    task.actions = ActionCounts {
        planned: 1,
        ..ActionCounts::default()
    };
    let details = vec![TaskDetailEntry {
        task_id: task.task_id.clone(),
        lines: vec!["would link: ~/.bashrc \u{2192} symlinks/bashrc".to_string()],
    }];

    assert_eq!(
        task_result_lines(&task, &details, plain_opts()),
        [
            "~ Install symlinks",
            "  link ~/.bashrc \u{2192} symlinks/bashrc"
        ]
    );
}

#[test]
fn task_result_lines_include_all_details() {
    let task = task_entry("large-plan", TaskStatus::DryRun, None);
    let details = vec![TaskDetailEntry {
        task_id: "large-plan".to_string(),
        lines: vec![
            (1..=11)
                .map(|index| format!("item {index}"))
                .collect::<Vec<String>>()
                .join("\n"),
        ],
    }];

    let lines = task_result_lines(&task, &details, plain_opts());

    assert_eq!(lines.len(), 12);
    assert_eq!(
        lines.first().expect("task status line should exist"),
        "~ large-plan"
    );
    assert_eq!(
        lines.last().expect("last detail line should exist"),
        "  item 11"
    );
}

#[test]
fn task_row_golden_matrix() {
    for (status, glyph, word, color, verbose_only) in [
        (TaskStatus::Changed, "✓", "CHANGE", "32", false),
        (TaskStatus::Passed, "✓", "PASSED", "32", false),
        (TaskStatus::DryRun, "~", "DRYRUN", "35", false),
        (TaskStatus::Skipped, "⊘", "SKIPPED", "33", false),
        (TaskStatus::Failed, "✗", "FAILED", "31", false),
        (TaskStatus::Blocked, "⊘", "BLOCKED", "33", false),
        (TaskStatus::Interrupted, "⊘", "INTERRUPTED", "33", false),
        (TaskStatus::Ok, "○", "OK", "2", true),
        (TaskStatus::NotApplicable, "⁃", "N/A", "2", true),
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
                        let mut row = if ansi {
                            format!(
                                "\x1b[{color}m{label}\x1b[0m \x1b[1mtask\x1b[0m\x1b[2m · reason\x1b[0m"
                            )
                        } else {
                            format!("{label} task · reason")
                        };
                        if verbose && status != TaskStatus::NotApplicable {
                            row.push_str(if ansi {
                                "\x1b[2m · 1.5s\x1b[0m"
                            } else {
                                " · 1.5s"
                            });
                        }
                        let expected: Vec<_> = (!verbose_only || verbose)
                            .then_some(row)
                            .into_iter()
                            .collect();
                        assert_eq!(
                            task_result_lines(
                                &task,
                                &[],
                                RowOpts {
                                    mode,
                                    symbols,
                                    verbose,
                                    style: StyleChoice::auto(ansi, false),
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
        false,
        "1.0s",
        StyleChoice::colored(),
    );

    assert_eq!(
        lines,
        ["\x1b[1m\x1b[31m4 failed\x1b[0m\x1b[0m \
             \x1b[2m·\x1b[0m \x1b[32m1 changed\x1b[0m \
             \x1b[2m·\x1b[0m \x1b[2m2 current\x1b[0m \
             \x1b[2m·\x1b[0m \x1b[33m3 skipped\x1b[0m \
             \x1b[2m·\x1b[0m \x1b[2m1.0s\x1b[0m"]
    );
}

#[test]
fn print_summary_clears_visible_progress() {
    let (log, _tmp, _guard) = crate::infra::logging::isolated_logger();
    record_task(&log, "changed-task", TaskStatus::Changed, None);
    log.notify_task_start_with_progress("active-task", true);

    assert!(log.has_transient_rows());
    assert!(log.has_status_row());

    log.print_summary();

    assert!(!log.has_transient_rows());
    assert!(!log.has_status_row());
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
fn install_summary_needs_totals_separator_after_task_output() {
    let (mut log, _tmp, _guard) = crate::infra::logging::isolated_logger_for("install");
    log.set_verbose(false);
    record_task(&log, "task-changed", TaskStatus::Changed, None);
    log.mark_task_console_output();

    assert!(log.needs_totals_separator());
}

#[test]
fn blocked_and_interrupted_counts_do_not_become_skips_or_failures() {
    let tasks = [
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
        format_summary_lines(
            counts,
            SummaryMode::Check,
            false,
            "1.0s",
            StyleChoice::plain()
        ),
        ["1 blocked · 1 interrupted · 1 skipped · 1.0s"]
    );
}
