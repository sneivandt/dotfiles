//! Per-task line rendering for the end-of-run summary.
//!
//! Converts a recorded task plus its buffered detail lines into the console
//! rows shown beneath (or in place of) the aggregate totals.

use super::status;
use super::totals::SummaryMode;
use crate::infra::logging::logger::TaskDetailEntry;
use crate::infra::logging::style::{StyleChoice, TextStyle};
use crate::infra::logging::types::{TaskEntry, TaskStatus};
use crate::infra::logging::utils::{
    compact_detail_line, duplicates_task_message, format_elapsed, is_stats_summary,
    sort_action_runs,
};

/// Width of the one-cell status glyph column.
const STATUS_WIDTH: usize = 1;

/// Rendering options for a single task row.
#[derive(Clone, Copy, Debug)]
pub(super) struct RowOpts {
    pub(super) mode: SummaryMode,
    pub(super) style: StyleChoice,
    pub(super) symbols: bool,
    /// Verbose rows show every task, per-task timing, and uncapped details.
    pub(super) verbose: bool,
}

pub(super) fn task_result_lines(
    task: &TaskEntry,
    details: &[TaskDetailEntry],
    opts: RowOpts,
) -> Vec<String> {
    if !task.visibility.is_visible() || !should_emit_task_result(task.status, opts.verbose) {
        return Vec::new();
    }

    let mut lines = vec![format_task_line(task, opts)];
    lines.extend(detail_rows(details, task, opts));
    lines
}

/// Render the indented action lines shown beneath a task's status row.
fn detail_rows(details: &[TaskDetailEntry], task: &TaskEntry, opts: RowOpts) -> Vec<String> {
    let mut lines: Vec<String> = task_detail_lines(details, task)
        .iter()
        .flat_map(|detail| detail.lines())
        .filter(|line| !line.trim().is_empty())
        .filter(|line| !is_stats_summary(line))
        .map(compact_detail_line)
        .collect();
    sort_action_runs(&mut lines, String::as_str);
    lines
        .iter()
        .map(|line| indented(line, opts.style))
        .collect()
}

fn indented(text: &str, style: StyleChoice) -> String {
    style.paint(TextStyle::Dim, &format!("  {}", text.trim_start()))
}

/// Whether a task produces a console row.
///
/// Non-verbose runs report only outcomes that need attention. Verbose runs
/// account for every task that ran, including the ones that had nothing to do,
/// so "why did this task not act?" is answerable from the console alone.
pub(super) const fn should_emit_task_result(status: TaskStatus, verbose: bool) -> bool {
    match status {
        TaskStatus::Changed
        | TaskStatus::Passed
        | TaskStatus::Skipped
        | TaskStatus::DryRun
        | TaskStatus::Failed => true,
        TaskStatus::Ok | TaskStatus::NotApplicable => verbose,
    }
}

/// The status text shown at the start of a task row.
fn status_text(status: TaskStatus, mode: SummaryMode, symbols: bool) -> String {
    if symbols {
        status::symbol(status).to_string()
    } else if matches!((status, mode), (TaskStatus::Changed, SummaryMode::Check)) {
        "PASSED".to_string()
    } else {
        status::word(status).to_string()
    }
}

/// Render one task row: status, name, then the reason and timing sections.
///
/// The task's message lives on this row rather than in the indented block
/// below it, so an indented line always means "an action this task took".
pub(super) fn format_task_line(task: &TaskEntry, opts: RowOpts) -> String {
    let padded = format!(
        "{:<STATUS_WIDTH$}",
        status_text(task.status, opts.mode, opts.symbols)
    );
    let mut line = format!(
        "{} {}",
        opts.style.paint(status::text_style(task.status), &padded),
        task.name
    );

    if let Some(reason) = row_reason(task) {
        line.push_str(
            &opts
                .style
                .paint(TextStyle::Dim, &format!(" \u{00b7} {reason}")),
        );
    }
    // A not-applicable task never ran, so its timing is either absent or a
    // meaningless `0.0s`. Suppressing it keeps `⁃` rows uniform whether the
    // task bailed out in `should_run()` or after being configured.
    if opts.verbose
        && task.status != TaskStatus::NotApplicable
        && let Some(duration) = task.duration
    {
        line.push_str(&opts.style.paint(
            TextStyle::Dim,
            &format!(" \u{00b7} {}", format_elapsed(duration)),
        ));
    }
    line
}

/// The reason section of a task row: the first line of the task message.
///
/// Later lines of a multi-line message (error chains, for example) stay in the
/// indented block so the row itself remains one screen line.
fn row_reason(task: &TaskEntry) -> Option<&str> {
    task.message
        .as_deref()
        .map(|message| message.lines().next().unwrap_or(message))
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .filter(|reason| !is_stats_summary(reason))
}

pub(super) fn task_detail_lines(details: &[TaskDetailEntry], task: &TaskEntry) -> Vec<String> {
    let task_message = task.message.as_deref();
    details
        .iter()
        .filter(|entry| {
            task.task_id.as_deref().map_or_else(
                || entry.task_id.is_none() && entry.name == task.name,
                |task_id| entry.task_id.as_deref() == Some(task_id),
            )
        })
        .flat_map(|entry| entry.lines.iter())
        .filter(|line| !duplicates_task_message(line, task_message))
        .filter(|line| Some(line.as_str()) != row_reason(task))
        .filter(|line| !is_stats_summary(line))
        .cloned()
        .collect()
}
