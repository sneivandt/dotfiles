//! Completed-task console rendering.
//!
//! Converts a recorded task plus its buffered detail lines into the console
//! rows emitted when that task finishes.

use super::status;
use super::totals::SummaryMode;
use crate::infra::logging::buffered::entry::{LogEntry, should_record_task_details};
use crate::infra::logging::style::{StyleChoice, TextStyle};
use crate::infra::logging::types::{MsgKind, TaskEntry, TaskResultDisplay, TaskStatus};
use crate::infra::logging::utils::{duplicates_task_message, format_elapsed};

/// Rendering options for a single task row.
#[derive(Clone, Copy, Debug)]
pub(super) struct RowOpts {
    pub(super) mode: SummaryMode,
    pub(super) style: StyleChoice,
    pub(super) symbols: bool,
    /// Verbose rows show every applicable task, per-task timing, and uncapped details.
    pub(super) verbose: bool,
}

/// A completed task's status and ordered details, ready for console output.
pub(super) struct TaskBlock {
    pub(super) status: Option<String>,
    pub(super) details: Vec<(MsgKind, String)>,
    pub(super) expanded: bool,
}

pub(super) fn task_block(task: &TaskEntry, entries: &[LogEntry], opts: RowOpts) -> TaskBlock {
    let mut block = TaskBlock {
        status: None,
        details: Vec::new(),
        expanded: opts.verbose,
    };
    if !task.visibility.is_visible() || task.status == TaskStatus::NotApplicable {
        return block;
    }
    let message = task.message.as_deref();
    if opts.verbose {
        if !task.is_unstarted_interruption() {
            block.status = Some(format_task_line(task, opts));
        }
        block.details = entries
            .iter()
            .filter_map(|entry| entry.verbose_detail(message))
            .map(|(kind, text)| (kind, text.to_string()))
            .collect();
    } else {
        let details = if should_record_task_details(task.status) {
            entries
                .iter()
                .filter_map(|entry| entry.detail_line(task.status))
                .map(str::to_string)
                .collect()
        } else {
            Vec::new()
        };
        let mut rows = task_result_lines(task, &details, opts).into_iter();
        block.status = rows.next();
        block.details = rows.map(|row| (MsgKind::Always, row)).collect();
        if task.status != TaskStatus::Ok {
            block.details.extend(
                entries
                    .iter()
                    .filter(|entry| entry.is_visible_in_non_verbose(task.status, message))
                    .map(|entry| (entry.kind(), entry.message().to_string())),
            );
        }
        block.expanded = !block.details.is_empty();
    }
    block
}

pub(super) fn task_result_lines(
    task: &TaskEntry,
    details: &[String],
    opts: RowOpts,
) -> Vec<String> {
    if !task.visibility.is_visible()
        || task.is_unstarted_interruption()
        || (task.result_display == TaskResultDisplay::RestartNotice
            && task.status == TaskStatus::Changed)
        || !should_emit_task_result(task.status, opts.verbose)
    {
        return Vec::new();
    }

    let detail_rows = detail_rows(details, task, opts);
    let show_reason = detail_rows.is_empty()
        || matches!(
            task.status,
            TaskStatus::Failed
                | TaskStatus::Skipped
                | TaskStatus::Blocked
                | TaskStatus::Interrupted
                | TaskStatus::NotApplicable
        );
    let mut lines = vec![format_task_line_with_reason(task, opts, show_reason)];
    lines.extend(detail_rows);
    lines
}

/// Render the indented action lines shown beneath a task's status row.
fn detail_rows(details: &[String], task: &TaskEntry, opts: RowOpts) -> Vec<String> {
    task_detail_lines(details, task)
        .iter()
        .flat_map(|detail| detail.lines())
        .filter(|line| !line.trim().is_empty())
        .map(|line| indented(line.trim_start(), opts.style))
        .collect()
}

fn indented(text: &str, style: StyleChoice) -> String {
    style.clean(&format!("  {}", text.trim_start()))
}

/// Whether a task produces a console row.
///
/// Non-verbose runs report only outcomes that need attention. Verbose runs also
/// report current tasks, but tasks that do not apply to this host stay out of
/// the console in both modes.
pub(super) const fn should_emit_task_result(status: TaskStatus, verbose: bool) -> bool {
    match status {
        TaskStatus::Changed
        | TaskStatus::Passed
        | TaskStatus::Skipped
        | TaskStatus::Blocked
        | TaskStatus::Interrupted
        | TaskStatus::DryRun
        | TaskStatus::Failed => true,
        TaskStatus::Ok => verbose,
        TaskStatus::NotApplicable => false,
    }
}

/// Render one task row: status, name, then the reason and timing sections.
///
/// The task's message lives on this row unless a successful outcome already
/// lists its concrete actions below it.
pub(super) fn format_task_line(task: &TaskEntry, opts: RowOpts) -> String {
    format_task_line_with_reason(task, opts, true)
}

fn format_task_line_with_reason(task: &TaskEntry, opts: RowOpts, show_reason: bool) -> String {
    let (label, text_style) = status::presentation(task.status, opts.mode, opts.symbols);
    let mut line = format!(
        "{} {}",
        opts.style.paint(text_style, label),
        opts.style.paint(TextStyle::Bold, &task.name)
    );

    if show_reason && let Some(reason) = row_reason(task) {
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
    if task.message_is_summary {
        return None;
    }
    task.message
        .as_deref()
        .map(|message| message.lines().next().unwrap_or(message))
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
}

pub(super) fn task_detail_lines(details: &[String], task: &TaskEntry) -> Vec<String> {
    let task_message = task.message.as_deref();
    details
        .iter()
        .filter(|line| !duplicates_task_message(line, task_message))
        .filter(|line| Some(line.as_str()) != row_reason(task))
        .cloned()
        .collect()
}
