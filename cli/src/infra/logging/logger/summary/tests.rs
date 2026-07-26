use std::time::Duration;

use super::render::{format_task_line, task_detail_lines, task_result_lines};
use super::totals::{SummaryCounts, SummaryMode, format_summary_lines, should_space_before_totals};
use crate::infra::logging::logger::TaskDetailEntry;
use crate::infra::logging::style::StyleChoice;
use crate::infra::logging::types::{ActionCounts, TaskEntry, TaskStatus};
use crate::infra::logging::utils::format_elapsed;

#[test]
fn format_elapsed_values() {
    assert_eq!(format_elapsed(Duration::from_millis(450)), "0.5s");
    assert_eq!(format_elapsed(Duration::from_secs_f64(3.7)), "3.7s");
    assert_eq!(format_elapsed(Duration::from_secs(125)), "2m 5s");
}

#[test]
fn standard_no_op_has_only_already_up_to_date_line() {
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
        ["Failed · Applied 87 changes across 3 tasks · 1 ignored · 1 failed · 2.0s"]
    );
}

#[test]
fn preview_summary_uses_planned_vocabulary() {
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

    assert_eq!(lines, ["81 changes planned across 1 task · 0.8s"]);
}

#[test]
fn preview_summary_counts_unquantified_work_as_tasks() {
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

    assert_eq!(lines, ["2 tasks planned · 0.8s"]);
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

    assert_eq!(lines, ["Changed 2 tasks · 1.0s"]);
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
            !should_space_before_totals(command, false, false),
            "{command} no-op runs should not add an extra separator"
        );
    }
    assert!(should_space_before_totals("install", false, true));
    assert!(should_space_before_totals("install", true, false));
    assert!(should_space_before_totals("test", false, false));
}

#[test]
fn changed_task_line_uses_fixed_width_status() {
    let task = TaskEntry {
        name: "symlinks".to_string(),
        status: TaskStatus::Changed,
        message: Some("3 changed, 8 already ok".to_string()),
        visible: true,
        actions: ActionCounts::default(),
    };

    assert_eq!(
        format_task_line(&task, SummaryMode::Standard, StyleChoice::colored()),
        "\x1b[32mCHANGE\x1b[0m symlinks"
    );
    assert_eq!(
        format_task_line(&task, SummaryMode::Standard, StyleChoice::plain()),
        "CHANGE symlinks"
    );
}

#[test]
fn task_detail_lines_filters_generic_stats_summary() {
    let task = TaskEntry {
        name: "symlinks".to_string(),
        status: TaskStatus::Changed,
        message: Some("2 changed, 1 already ok".to_string()),
        visible: true,
        actions: ActionCounts::default(),
    };
    let details = vec![TaskDetailEntry {
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
fn task_detail_lines_filters_prefixed_skip_message() {
    let task = TaskEntry {
        name: "skip-task".to_string(),
        status: TaskStatus::Skipped,
        message: Some("dependency failed".to_string()),
        visible: true,
        actions: ActionCounts::default(),
    };
    let details = vec![TaskDetailEntry {
        name: "skip-task".to_string(),
        lines: vec!["skipped: dependency failed".to_string()],
    }];

    assert_eq!(
        task_detail_lines(&details, &task),
        vec!["dependency failed"]
    );
}

#[test]
fn task_detail_lines_keeps_custom_message_when_no_details_exist() {
    let task = TaskEntry {
        name: "custom task".to_string(),
        status: TaskStatus::Changed,
        message: Some("generated private config".to_string()),
        visible: true,
        actions: ActionCounts::default(),
    };

    assert_eq!(
        task_detail_lines(&[], &task),
        vec!["generated private config"]
    );
}

#[test]
fn task_result_lines_are_flat_with_reduced_indent() {
    let task = TaskEntry {
        name: "changed-task".to_string(),
        status: TaskStatus::Changed,
        message: None,
        visible: true,
        actions: ActionCounts::default(),
    };
    let details = vec![TaskDetailEntry {
        name: "changed-task".to_string(),
        lines: vec!["linked: ~/.example".to_string()],
    }];

    assert_eq!(
        task_result_lines(
            &task,
            &details,
            SummaryMode::Standard,
            StyleChoice::colored(),
        ),
        vec![
            "\x1b[32mCHANGE\x1b[0m changed-task",
            "\x1b[2m  link ~/.example\x1b[0m"
        ]
    );
}

#[test]
fn task_result_lines_abbreviate_symlink_actions() {
    let task = TaskEntry {
        name: "Install symlinks".to_string(),
        status: TaskStatus::DryRun,
        message: None,
        visible: true,
        actions: ActionCounts {
            planned: 1,
            ..ActionCounts::default()
        },
    };
    let details = vec![TaskDetailEntry {
        name: task.name.clone(),
        lines: vec!["would link: ~/.bashrc \u{2190} symlinks/bashrc".to_string()],
    }];

    assert_eq!(
        task_result_lines(&task, &details, SummaryMode::Standard, StyleChoice::plain(),),
        [
            "DRYRUN Install symlinks",
            "  link ~/.bashrc \u{2190} symlinks/bashrc"
        ]
    );
}

#[test]
fn task_result_lines_bound_non_verbose_details() {
    let task = TaskEntry {
        name: "large-plan".to_string(),
        status: TaskStatus::DryRun,
        message: None,
        visible: true,
        actions: ActionCounts::default(),
    };
    let details = vec![TaskDetailEntry {
        name: "large-plan".to_string(),
        lines: vec![
            (1..=11)
                .map(|index| format!("item {index}"))
                .collect::<Vec<String>>()
                .join("\n"),
        ],
    }];

    let lines = task_result_lines(&task, &details, SummaryMode::Standard, StyleChoice::plain());

    assert_eq!(lines.len(), 10);
    assert_eq!(
        lines.first().expect("task status line should exist"),
        "DRYRUN large-plan"
    );
    assert_eq!(
        lines.get(8).expect("eighth detail line should exist"),
        "  item 8"
    );
    assert_eq!(
        lines.get(9).expect("overflow detail line should exist"),
        "  … 3 more; use -v for the full plan"
    );
}

#[test]
fn task_result_lines_skip_unchanged_tasks() {
    let task = TaskEntry {
        name: "unchanged-task".to_string(),
        status: TaskStatus::Ok,
        message: None,
        visible: true,
        actions: ActionCounts::default(),
    };

    assert!(
        task_result_lines(&task, &[], SummaryMode::Standard, StyleChoice::colored()).is_empty()
    );
}

#[test]
fn validation_task_line_uses_passed_status() {
    let task = TaskEntry {
        name: "Validate config".to_string(),
        status: TaskStatus::Passed,
        message: None,
        visible: true,
        actions: ActionCounts::default(),
    };

    assert_eq!(
        format_task_line(&task, SummaryMode::Test, StyleChoice::plain()),
        "PASSED Validate config"
    );
}

#[test]
fn colored_summary_does_not_bold_completion_or_group_labels() {
    let lines = format_summary_lines(
        SummaryCounts {
            changed: 1,
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

    assert!(
        lines.iter().all(|line| !line.contains("\x1b[1m")),
        "completion and totals labels should not be bold"
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
