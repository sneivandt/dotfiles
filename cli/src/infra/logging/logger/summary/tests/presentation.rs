use super::*;

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
        [
            "\x1b[2mTasks\x1b[0m \x1b[2m·\x1b[0m \x1b[1;31m4 failed\x1b[0m \
             \x1b[2m·\x1b[0m \x1b[1;33m3 skipped\x1b[0m \
             \x1b[2m·\x1b[0m \x1b[32m1 changed\x1b[0m \
             \x1b[2m·\x1b[0m \x1b[2m2 unchanged\x1b[0m \
             \x1b[2m·\x1b[0m \x1b[2m1.0s\x1b[0m"
        ]
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
                counts!(skipped: 1),
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
                let lines = summary_lines_with_disposition(
                    counts,
                    mode,
                    "1.0s",
                    StyleChoice::auto(ansi, false),
                    disposition,
                );
                let clean = crate::infra::logging::utils::strip_ansi(&lines[0]);
                let (prefix, styled_prefix) = match (counts.skipped > 0, mode) {
                    (true, SummaryMode::Check) => ("Checks: ", "\x1b[2mChecks:\x1b[0m "),
                    (true, SummaryMode::Standard) => {
                        ("Tasks · ", "\x1b[2mTasks\x1b[0m \x1b[2m·\x1b[0m ")
                    }
                    (false, _) => ("", ""),
                };
                assert_eq!(clean, format!("{prefix}{text} · 1.0s"));
                if ansi {
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
