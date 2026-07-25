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
            if !task.visible {
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

    pub(super) const fn has_visible_tasks(self) -> bool {
        self.changed > 0
            || self.passed > 0
            || self.ok > 0
            || self.skipped > 0
            || self.dry_run > 0
            || self.failed > 0
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
    if counts.failed > 0 {
        parts.push(style.paint(TextStyle::Red, "Failed"));
        if counts.actions.applied > 0 {
            parts.push(style.paint(
                TextStyle::Green,
                &format!(
                    "Applied {} {} across {} {}",
                    counts.actions.applied,
                    pluralize(counts.actions.applied, "change", "changes"),
                    counts.changed,
                    pluralize(counts.changed, "task", "tasks")
                ),
            ));
        } else if counts.changed > 0 {
            parts.push(style.paint(
                TextStyle::Green,
                &format!(
                    "Changed {} {}",
                    counts.changed,
                    pluralize(counts.changed, "task", "tasks")
                ),
            ));
        }
    } else if dry_run && counts.actions.planned > 0 {
        parts.push(style.paint(
            TextStyle::Magenta,
            &format!(
                "{} {} planned across {} {}",
                counts.actions.planned,
                pluralize(counts.actions.planned, "change", "changes"),
                counts.dry_run,
                pluralize(counts.dry_run, "task", "tasks")
            ),
        ));
    } else if counts.actions.applied > 0 {
        parts.push(style.paint(
            TextStyle::Green,
            &format!(
                "Applied {} {} across {} {}",
                counts.actions.applied,
                pluralize(counts.actions.applied, "change", "changes"),
                counts.changed,
                pluralize(counts.changed, "task", "tasks")
            ),
        ));
    } else if counts.changed > 0 {
        parts.push(style.paint(
            TextStyle::Green,
            &format!(
                "Changed {} {}",
                counts.changed,
                pluralize(counts.changed, "task", "tasks")
            ),
        ));
    } else if counts.dry_run > 0 {
        parts.push(style.paint(
            TextStyle::Magenta,
            &format!(
                "{} {} planned",
                counts.dry_run,
                pluralize(counts.dry_run, "task", "tasks")
            ),
        ));
    } else {
        parts.push("No changes".to_string());
    }
    push_count(&mut parts, counts.ok, TextStyle::Dim, "up to date", style);
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

pub(super) const fn pluralize(
    count: u32,
    singular: &'static str,
    plural: &'static str,
) -> &'static str {
    if count == 1 { singular } else { plural }
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

pub(super) fn should_space_before_totals(
    command: &str,
    verbose: bool,
    has_visible_tasks: bool,
) -> bool {
    verbose || has_visible_tasks || !matches!(command, "install" | "update" | "uninstall")
}
