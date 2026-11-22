use super::*;

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
                format_summary_lines(counts!(ok: count), SummaryMode::Standard, "3.4s", style,),
                [format!(
                    "{} {} {} {} {}",
                    style.paint(crate::infra::logging::style::TextStyle::Dim, "Tasks"),
                    style.paint(crate::infra::logging::style::TextStyle::Dim, "·"),
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
            ["Tasks · 1 interrupted · 1.0s"],
        );
    }
}

#[test]
fn standard_summary_totals_preserve_task_and_action_counts() {
    for (case, counts, elapsed, expected) in [
        (
            "standard_error_summary_starts_with_failed_count",
            counts!(ok: 1, failed: 1),
            "7.2s",
            "Tasks · 1 failed · 1 unchanged · 7.2s",
        ),
        (
            "standard_summary_groups_task_and_action_counts",
            SummaryCounts {
                actions: ActionCounts {
                    applied: 87,
                    skipped: 2,
                    failed: 1,
                    ..ActionCounts::default()
                },
                ..counts!(changed: 3, skipped: 1, failed: 1)
            },
            "2.0s",
            "Tasks · 1 failed · 1 skipped · 3 changed · 2.0s",
        ),
        (
            "dry_run_summary_pairs_affected_and_planned_counts",
            SummaryCounts {
                actions: ActionCounts {
                    planned: 81,
                    ..ActionCounts::default()
                },
                ..counts!(dry_run: 1)
            },
            "0.8s",
            "Tasks · 1 planned · 0.8s",
        ),
        (
            "dry_run_summary_counts_unquantified_affected_tasks",
            counts!(dry_run: 2),
            "0.8s",
            "Tasks · 2 planned · 0.8s",
        ),
        (
            "summary_totals_account_for_every_reported_task",
            SummaryCounts {
                actions: ActionCounts {
                    applied: 4,
                    ..ActionCounts::default()
                },
                ..counts!(changed: 2, ok: 15, skipped: 1)
            },
            "2.3s",
            "Tasks · 1 skipped · 2 changed · 15 unchanged · 2.3s",
        ),
        (
            "standard_summary_omits_actions_when_all_action_counts_are_zero",
            counts!(changed: 2),
            "1.0s",
            "Tasks · 2 changed · 1.0s",
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
    let counts = counts!(passed: 7, skipped: 2, failed: 1);
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
fn unstarted_interruptions_do_not_produce_rows_or_totals() {
    let queued = task_entry("queued", TaskStatus::Interrupted, Some("cancelled"));
    let mut started = task_entry("started", TaskStatus::Interrupted, Some("interrupted"));
    started.duration = Some(Duration::from_millis(100));

    assert_hidden_row(&queued);
    assert_eq!(
        task_result_lines(&started, &[], plain_opts()),
        ["⊘ started · interrupted"]
    );

    let counts = SummaryCounts::from_tasks(&[queued, started]);
    assert_eq!(counts.interrupted, 1);
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
            "\x1b[2mTasks\x1b[0m \x1b[2m·\x1b[0m \x1b[1;31m1 failed\x1b[0m \x1b[2m·\x1b[0m \x1b[1;33m2 interrupted\x1b[0m \x1b[2m·\x1b[0m \x1b[1;33m3 blocked\x1b[0m \x1b[2m·\x1b[0m \x1b[1;33m4 skipped\x1b[0m \x1b[2m·\x1b[0m \x1b[32m5 changed\x1b[0m \x1b[2m·\x1b[0m \x1b[35m6 planned\x1b[0m \x1b[2m·\x1b[0m \x1b[2m7 unchanged\x1b[0m \x1b[2m·\x1b[0m \x1b[2m1.0s\x1b[0m"
        ]
    );
}

#[test]
fn incomplete_summaries_report_each_outcome_without_duplicate_disposition_text() {
    for (counts, incomplete_disposition, text) in [
        (
            counts!(failed: 1),
            CompletionDisposition::Failed,
            "1 failed",
        ),
        (
            counts!(interrupted: 1),
            CompletionDisposition::Interrupted,
            "1 interrupted",
        ),
        (
            counts!(blocked: 1),
            CompletionDisposition::Succeeded,
            "1 blocked",
        ),
        (
            counts!(skipped: 1),
            CompletionDisposition::Succeeded,
            "1 skipped",
        ),
    ] {
        for mode in [SummaryMode::Standard, SummaryMode::Check] {
            for disposition in [CompletionDisposition::Succeeded, incomplete_disposition] {
                let lines = summary_lines_with_disposition(
                    counts,
                    mode,
                    "1.0s",
                    StyleChoice::plain(),
                    disposition,
                );
                let prefix = match mode {
                    SummaryMode::Check => "Checks: ",
                    SummaryMode::Standard => "Tasks · ",
                };
                assert_eq!(
                    lines,
                    [format!("{prefix}{text} · 1.0s")],
                    "{mode:?}, {disposition:?}"
                );
            }
        }
    }
}
