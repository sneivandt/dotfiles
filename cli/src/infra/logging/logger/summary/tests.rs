use std::time::Duration;

use super::render::{
    RowOpts, format_task_line, should_emit_task_result, task_detail_lines, task_result_lines,
};
use super::totals::{SummaryCounts, SummaryMode, format_summary_lines, should_space_before_totals};
use crate::infra::logging::logger::TaskDetailEntry;
use crate::infra::logging::style::StyleChoice;
use crate::infra::logging::types::{ActionCounts, TaskEntry, TaskStatus, TaskVisibility};
use crate::infra::logging::utils::format_elapsed;

/// Build a task entry with the fields a row-rendering test cares about.
fn task_entry(name: &str, status: TaskStatus, message: Option<&str>) -> TaskEntry {
    TaskEntry {
        task_id: None,
        name: name.to_string(),
        status,
        message: message.map(str::to_string),
        visibility: TaskVisibility::Visible,
        actions: ActionCounts::default(),
        duration: None,
    }
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
    let lines = format_summary_lines(
        SummaryCounts::default(),
        SummaryMode::Standard,
        false,
        "1.2s",
        StyleChoice::plain(),
    );

    assert_eq!(lines, ["No changes · 1.2s"]);
}

#[test]
fn standard_summary_groups_task_and_action_counts() {
    let lines = format_summary_lines(
        SummaryCounts {
            changed: 3,
            passed: 0,
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

    assert_eq!(
        lines,
        ["3 changed (87 applied) · 1 ignored · 1 failed · 2.0s"]
    );
}

#[test]
fn dry_run_summary_pairs_affected_and_planned_counts() {
    let lines = format_summary_lines(
        SummaryCounts {
            changed: 0,
            passed: 0,
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

    assert_eq!(lines, ["1 would change (81 planned) · 0.8s"]);
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
        ["2 changed (4 applied) \u{b7} 15 current \u{b7} 1 ignored \u{b7} 2.3s"],
        "every task the run reported on must be represented in the totals"
    );
}

#[test]
fn standard_summary_omits_actions_when_all_action_counts_are_zero() {
    let lines = format_summary_lines(
        SummaryCounts {
            changed: 2,
            passed: 0,
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
fn test_summary_uses_check_vocabulary_and_omits_not_run() {
    let lines = format_summary_lines(
        SummaryCounts {
            changed: 0,
            passed: 7,
            ok: 0,
            skipped: 2,
            dry_run: 0,
            failed: 1,
            actions: ActionCounts::default(),
        },
        SummaryMode::Test,
        false,
        "3.4s",
        StyleChoice::plain(),
    );

    assert_eq!(lines, ["7 passed · 2 ignored · 1 failed · 3.4s"]);
}

#[test]
fn no_op_standard_commands_skip_extra_blank() {
    for command in ["install", "update", "uninstall"] {
        assert!(
            !should_space_before_totals(command, false),
            "{command} no-op runs should not add an extra separator"
        );
    }
    assert!(should_space_before_totals("install", true));
    assert!(should_space_before_totals("test", false));
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
        "\x1b[32m✓\x1b[0m symlinks"
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

    assert_eq!(format_task_line(&task, opts), "‧ Home symlinks \u{b7} 1.5s");
}

#[test]
fn task_detail_lines_filters_generic_stats_summary() {
    let task = task_entry(
        "symlinks",
        TaskStatus::Changed,
        Some("2 changed, 1 already ok"),
    );
    let details = vec![TaskDetailEntry {
        task_id: None,
        name: "symlinks".to_string(),
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
        task_id: None,
        name: "skip-task".to_string(),
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
        task_id: None,
        name: "changed-task".to_string(),
        lines: vec!["linked: ~/.example".to_string()],
    }];

    assert_eq!(
        task_result_lines(&task, &details, colored_opts()),
        vec![
            "\x1b[32m✓\x1b[0m changed-task",
            "\x1b[2m  link ~/.example\x1b[0m"
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
        task_id: None,
        name: task.name.clone(),
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
        task_id: None,
        name: "large-plan".to_string(),
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
fn task_result_lines_skip_unchanged_tasks() {
    let task = task_entry("unchanged-task", TaskStatus::Ok, None);

    assert!(task_result_lines(&task, &[], colored_opts()).is_empty());
}

#[test]
fn verbose_task_result_lines_account_for_unchanged_tasks() {
    let task = task_entry("unchanged-task", TaskStatus::Ok, None);
    let opts = RowOpts {
        verbose: true,
        ..plain_opts()
    };

    assert_eq!(task_result_lines(&task, &[], opts), ["‧ unchanged-task"]);
}

#[test]
fn validation_task_line_uses_passed_status() {
    let task = task_entry("Validate config", TaskStatus::Passed, None);
    let opts = RowOpts {
        mode: SummaryMode::Test,
        ..plain_opts()
    };

    assert_eq!(format_task_line(&task, opts), "✓ Validate config");
}

#[test]
fn task_line_uses_words_when_symbols_are_disabled() {
    let task = task_entry("symlinks", TaskStatus::Changed, None);
    let opts = RowOpts {
        symbols: false,
        ..plain_opts()
    };

    assert_eq!(format_task_line(&task, opts), "CHANGE symlinks");
}

#[test]
fn task_result_visibility_is_unchanged_for_every_status() {
    for status in [
        TaskStatus::Changed,
        TaskStatus::DryRun,
        TaskStatus::Passed,
        TaskStatus::Skipped,
        TaskStatus::Failed,
    ] {
        assert!(should_emit_task_result(status, false));
        assert!(should_emit_task_result(status, true));
    }
    for status in [TaskStatus::Ok, TaskStatus::NotApplicable] {
        assert!(!should_emit_task_result(status, false));
        assert!(should_emit_task_result(status, true));
    }
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
        ["\x1b[32m1 changed (2 applied)\x1b[0m \
             \x1b[2m·\x1b[0m \x1b[2m2 current\x1b[0m \
             \x1b[2m·\x1b[0m \x1b[33m3 ignored\x1b[0m \
             \x1b[2m·\x1b[0m \x1b[31m4 failed\x1b[0m \
             \x1b[2m·\x1b[0m \x1b[2m1.0s\x1b[0m"]
    );
}

#[test]
fn print_summary_clears_visible_progress() {
    let (log, _tmp, _guard) = crate::infra::logging::isolated_logger();
    log.record_task("changed-task", TaskStatus::Changed, None);
    log.notify_task_start_with_progress("active-task", true);

    assert!(log.has_transient_rows());
    assert!(log.has_status_row());

    log.print_summary();

    assert!(!log.has_transient_rows());
    assert!(!log.has_status_row());
}

#[test]
fn no_op_update_summary_needs_no_totals_separator() {
    let (mut log, _tmp, _guard) = crate::infra::logging::isolated_logger_for("update");
    log.set_verbose(false);
    for index in 0..3 {
        log.record_task(&format!("task-{index}"), TaskStatus::Ok, None);
    }

    assert!(
        !log.needs_totals_separator(),
        "runs that printed nothing should not emit a blank line before the totals"
    );
}

#[test]
fn update_summary_needs_totals_separator_after_task_output() {
    let (mut log, _tmp, _guard) = crate::infra::logging::isolated_logger_for("update");
    log.set_verbose(false);
    log.record_task("task-changed", TaskStatus::Changed, None);
    log.mark_task_console_output();

    assert!(log.needs_totals_separator());
}
