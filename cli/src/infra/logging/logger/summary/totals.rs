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
    for (count, label, color) in [
        (counts.failed, "failed", TextStyle::Red),
        (counts.interrupted, "interrupted", TextStyle::Yellow),
        (counts.blocked, "blocked", TextStyle::Yellow),
        (counts.skipped, "skipped", TextStyle::Yellow),
    ] {
        push_count(&mut parts, count, color, label, style, true);
    }
    match mode {
        SummaryMode::Standard => {
            push_count(
                &mut parts,
                counts.changed,
                TextStyle::Green,
                "changed",
                style,
                false,
            );
            push_count(
                &mut parts,
                counts.dry_run,
                TextStyle::Magenta,
                "planned",
                style,
                false,
            );
            let unchanged_style = if parts.is_empty() {
                TextStyle::Bold
            } else {
                TextStyle::Dim
            };
            push_count(
                &mut parts,
                counts.ok,
                unchanged_style,
                "unchanged",
                style,
                false,
            );
        }
        SummaryMode::Check => {
            push_count(
                &mut parts,
                counts.passed,
                TextStyle::Green,
                "passed",
                style,
                false,
            );
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

/// Emphasize the first outcome plus all incomplete work.
fn push_count(
    parts: &mut Vec<String>,
    count: u32,
    text_style: TextStyle,
    label: &str,
    style: StyleChoice,
    incomplete: bool,
) {
    if count > 0 {
        let text = format!("{count} {label}");
        parts.push(if matches!(text_style, TextStyle::Bold) {
            style.bold(&text)
        } else if incomplete || parts.is_empty() {
            style.paint_bold(text_style, &text)
        } else {
            style.paint(text_style, &text)
        });
    }
}

/// Decide whether a blank line should separate the totals from what precedes
/// it. The separator is only useful when task output was emitted above the
/// totals. Standard mutation commands hide current task rows, so their no-op
/// runs reuse the startup separator.
pub(super) fn should_space_before_totals(command: &str, task_output_emitted: bool) -> bool {
    task_output_emitted || !matches!(command, "install" | "update" | "uninstall" | "remove")
}
