//! Aggregate counting and totals formatting for the end-of-run summary.
//!
//! Owns the summary mode, the visible-task tally, and the single totals line
//! rendered at the end of a run.

use crate::infra::logging::style::{StyleChoice, TextStyle};
use crate::infra::logging::types::{ActionCounts, TaskEntry, TaskStatus};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SummaryMode {
    Standard,
    Test,
}

impl SummaryMode {
    pub(super) fn for_command(command: &str) -> Self {
        if command == "test" {
            Self::Test
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
    pub(super) dry_run: u32,
    pub(super) failed: u32,
    pub(super) actions: ActionCounts,
}

impl SummaryCounts {
    pub(super) fn from_tasks(tasks: &[TaskEntry]) -> Self {
        let mut counts = Self::default();
        for task in tasks {
            if !task.visibility.is_visible() {
                continue;
            }
            match task.status {
                TaskStatus::Changed => counts.changed = counts.changed.saturating_add(1),
                TaskStatus::Passed => counts.passed = counts.passed.saturating_add(1),
                TaskStatus::Ok => counts.ok = counts.ok.saturating_add(1),
                TaskStatus::NotApplicable => {}
                TaskStatus::Skipped => counts.skipped = counts.skipped.saturating_add(1),
                TaskStatus::DryRun => counts.dry_run = counts.dry_run.saturating_add(1),
                TaskStatus::Failed => counts.failed = counts.failed.saturating_add(1),
            }
            counts.actions.merge(task.actions);
        }
        counts
    }
}

pub(super) fn format_summary_lines(
    counts: SummaryCounts,
    mode: SummaryMode,
    dry_run: bool,
    elapsed: &str,
    style: StyleChoice,
) -> Vec<String> {
    let mut parts = match mode {
        SummaryMode::Standard => format_standard_totals(counts, dry_run, style),
        SummaryMode::Test => format_test_totals(counts, style),
    };
    parts.push(style.paint(TextStyle::Dim, elapsed));
    vec![parts.join(&format!(" {} ", style.paint(TextStyle::Dim, "\u{00b7}")))]
}

pub(super) fn format_standard_totals(
    counts: SummaryCounts,
    dry_run: bool,
    style: StyleChoice,
) -> Vec<String> {
    let mut parts = Vec::new();
    if counts.dry_run > 0 || (dry_run && counts.actions.planned > 0) {
        parts.push(format_outcome_with_detail(
            counts.dry_run,
            "would change",
            counts.actions.planned,
            "planned",
            TextStyle::Magenta,
            style,
        ));
    } else if counts.changed > 0 {
        parts.push(format_outcome_with_detail(
            counts.changed,
            "changed",
            counts.actions.applied,
            "applied",
            TextStyle::Green,
            style,
        ));
    } else if counts.actions.applied > 0 {
        parts.push(style.paint(
            TextStyle::Green,
            &format!("{} applied", counts.actions.applied),
        ));
    } else if counts.failed == 0 {
        parts.push("No changes".to_string());
    }
    push_count(&mut parts, counts.ok, TextStyle::Dim, "current", style);
    push_count(
        &mut parts,
        counts.skipped,
        TextStyle::Yellow,
        "ignored",
        style,
    );
    push_count(&mut parts, counts.failed, TextStyle::Red, "failed", style);
    parts
}

fn format_outcome_with_detail(
    outcome_count: u32,
    outcome_label: &str,
    detail_count: u32,
    detail_label: &str,
    text_style: TextStyle,
    style: StyleChoice,
) -> String {
    let text = if outcome_count == 0 {
        format!("{detail_count} {detail_label}")
    } else if detail_count == 0 {
        format!("{outcome_count} {outcome_label}")
    } else {
        format!("{outcome_count} {outcome_label} ({detail_count} {detail_label})")
    };
    style.paint(text_style, &text)
}

/// Append a styled `"<count> <label>"` fragment, skipping zero counts.
pub(super) fn push_count(
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

pub(super) fn format_test_totals(counts: SummaryCounts, style: StyleChoice) -> Vec<String> {
    let mut parts = Vec::new();
    push_count(&mut parts, counts.passed, TextStyle::Green, "passed", style);
    push_count(
        &mut parts,
        counts.skipped,
        TextStyle::Yellow,
        "ignored",
        style,
    );
    push_count(&mut parts, counts.failed, TextStyle::Red, "failed", style);
    if parts.is_empty() {
        parts.push("No checks ran".to_string());
    }
    parts
}

/// Decide whether a blank line should separate the totals from what precedes
/// it.  The separator is only useful when console output was actually emitted
/// above the totals, so it is driven by emitted output rather than by recorded
/// task counts (tasks that were already up to date print nothing).  This holds
/// in verbose mode too: verbose emits a row per task, which sets
/// `task_output_emitted`, so it needs no separate carve-out.
pub(super) fn should_space_before_totals(command: &str, task_output_emitted: bool) -> bool {
    task_output_emitted || !matches!(command, "install" | "update" | "uninstall")
}
