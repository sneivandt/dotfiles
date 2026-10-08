//! Aggregate counting and totals formatting for the end-of-run summary.
//!
//! Owns the summary mode, the visible-task tally, and the single totals line
//! rendered at the end of a run.

use crate::infra::logging::style::{StyleChoice, TextStyle};
use crate::infra::logging::types::{ActionCounts, TaskEntry, TaskStatus};

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

pub(super) fn format_summary_lines(
    counts: SummaryCounts,
    mode: SummaryMode,
    elapsed: &str,
    style: StyleChoice,
) -> Vec<String> {
    let mut parts = Vec::new();
    push_count(&mut parts, counts.failed, TextStyle::Red, "failed", style);
    match mode {
        SummaryMode::Standard => {
            if counts.dry_run > 0 {
                push_count(
                    &mut parts,
                    counts.dry_run,
                    TextStyle::Magenta,
                    "would change",
                    style,
                );
            } else if counts.changed > 0 {
                push_count(
                    &mut parts,
                    counts.changed,
                    TextStyle::Green,
                    "changed",
                    style,
                );
            } else if counts.failed == 0
                && counts.actions.applied == 0
                && counts.actions.planned == 0
            {
                parts.push("No changes".to_string());
            }
            push_count(&mut parts, counts.ok, TextStyle::Dim, "current", style);
        }
        SummaryMode::Check => {
            push_count(&mut parts, counts.passed, TextStyle::Green, "passed", style);
        }
    }
    for (count, label) in [
        (counts.blocked, "blocked"),
        (counts.interrupted, "interrupted"),
        (counts.skipped, "skipped"),
    ] {
        push_count(&mut parts, count, TextStyle::Yellow, label, style);
    }
    if parts.is_empty() && mode == SummaryMode::Check {
        parts.push("No checks ran".to_string());
    }
    if let Some(outcome) = parts.first_mut() {
        *outcome = style.paint(TextStyle::Bold, outcome);
    }
    parts.push(style.paint(TextStyle::Dim, elapsed));
    vec![parts.join(&format!(" {} ", style.paint(TextStyle::Dim, "\u{00b7}")))]
}

/// Append a styled `"<count> <label>"` fragment, skipping zero counts.
fn push_count(
    parts: &mut Vec<String>,
    count: u32,
    text_style: TextStyle,
    label: &str,
    style: StyleChoice,
) {
    if count > 0 {
        parts.push(style.paint(text_style, &format!("{count} {label}")));
    }
}

/// Decide whether a blank line should separate the totals from what precedes
/// it. The separator is only useful when task output was emitted above the
/// totals. Standard mutation commands hide current task rows, so their no-op
/// runs reuse the startup separator.
pub(super) fn should_space_before_totals(command: &str, task_output_emitted: bool) -> bool {
    task_output_emitted || !matches!(command, "install" | "update" | "uninstall" | "remove")
}
