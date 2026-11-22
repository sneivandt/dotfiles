//! Aggregate counting and totals formatting for the end-of-run summary.
//!
//! Owns the summary mode, the visible-task tally, and the single totals line
//! rendered at the end of a run.

use crate::infra::logging::style::{StyleChoice, TextStyle};
use crate::infra::logging::types::{ActionCounts, CompletionDisposition, TaskEntry, TaskStatus};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SummaryMode {
    Standard,
    Check,
}

impl SummaryMode {
    pub(super) fn for_command(command: &str) -> Self {
        if command == "check" {
            Self::Check
        } else {
            Self::Standard
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct SummaryCounts {
    pub(super) changed: u32,
    pub(super) passed: u32,
    pub(super) ok: u32,
    pub(super) skipped: u32,
    pub(super) blocked: u32,
    pub(super) interrupted: u32,
    pub(super) dry_run: u32,
    pub(super) failed: u32,
    pub(super) actions: ActionCounts,
}

impl SummaryCounts {
    pub(super) fn from_tasks(tasks: &[TaskEntry]) -> Self {
        let mut counts = Self::default();
        for task in tasks {
            if !task.visibility.is_visible() || task.is_unstarted_interruption() {
                continue;
            }
            counts.actions.merge(task.actions);
            let counter = match task.status {
                TaskStatus::Changed => &mut counts.changed,
                TaskStatus::Passed => &mut counts.passed,
                TaskStatus::Ok => &mut counts.ok,
                TaskStatus::NotApplicable => continue,
                TaskStatus::Blocked => &mut counts.blocked,
                TaskStatus::Interrupted => &mut counts.interrupted,
                TaskStatus::Skipped => &mut counts.skipped,
                TaskStatus::DryRun => &mut counts.dry_run,
                TaskStatus::Failed => &mut counts.failed,
            };
            *counter = counter.saturating_add(1);
        }
        counts
    }
}

pub(super) fn format_summary_counts(
    counts: SummaryCounts,
    mode: SummaryMode,
    style: StyleChoice,
    disposition: CompletionDisposition,
) -> String {
    let mut parts = Vec::new();
    match disposition {
        CompletionDisposition::Failed if counts.failed == 0 => {
            parts.push(style.paint_bold(TextStyle::Red, "Failed"));
        }
        CompletionDisposition::Interrupted if counts.interrupted == 0 => {
            parts.push(style.paint_bold(TextStyle::Yellow, "Interrupted"));
        }
        CompletionDisposition::Succeeded
        | CompletionDisposition::Failed
        | CompletionDisposition::Interrupted => {}
    }
    let counts_start = parts.len();
    let incomplete = [
        (counts.failed, "failed", TextStyle::Red),
        (counts.interrupted, "interrupted", TextStyle::Yellow),
        (counts.blocked, "blocked", TextStyle::Yellow),
        (counts.skipped, "skipped", TextStyle::Yellow),
    ];
    let completed: &[_] = match mode {
        SummaryMode::Standard => &[
            (counts.changed, "changed", TextStyle::Green),
            (counts.dry_run, "planned", TextStyle::Magenta),
            (counts.ok, "unchanged", TextStyle::Dim),
        ],
        SummaryMode::Check => &[(counts.passed, "passed", TextStyle::Green)],
    };
    for (outcomes, emphasize) in [(&incomplete[..], true), (completed, false)] {
        for &(count, label, color) in outcomes {
            if count == 0 {
                continue;
            }
            let text = format!("{count} {label}");
            parts.push(if parts.is_empty() && matches!(color, TextStyle::Dim) {
                style.bold(&text)
            } else if emphasize || parts.is_empty() {
                style.paint_bold(color, &text)
            } else {
                style.paint(color, &text)
            });
        }
    }
    if let Some(first_count) = parts.get_mut(counts_start) {
        match mode {
            SummaryMode::Standard => {
                parts.insert(counts_start, style.paint(TextStyle::Dim, "Tasks"));
            }
            SummaryMode::Check => {
                *first_count = format!("{} {first_count}", style.paint(TextStyle::Dim, "Checks:"));
            }
        }
    }
    if parts.is_empty() {
        parts.push(style.paint(
            TextStyle::Bold,
            match mode {
                SummaryMode::Standard => "No applicable tasks",
                SummaryMode::Check => "No checks ran",
            },
        ));
    }
    parts.join(&format!(" {} ", style.paint(TextStyle::Dim, "\u{00b7}")))
}

pub(super) fn with_elapsed(counts: &str, elapsed: &str, style: StyleChoice) -> String {
    format!(
        "{counts} {} {}",
        style.paint(TextStyle::Dim, "\u{00b7}"),
        style.paint(TextStyle::Dim, elapsed)
    )
}

/// Decide whether a blank line should separate the totals from what precedes
/// it. The separator is only useful when task output was emitted above the
/// totals. Standard mutation commands hide current task rows, so their no-op
/// runs reuse the startup separator.
pub(super) fn should_space_before_totals(command: &str, task_output_emitted: bool) -> bool {
    task_output_emitted || !matches!(command, "install" | "update" | "uninstall" | "remove")
}
