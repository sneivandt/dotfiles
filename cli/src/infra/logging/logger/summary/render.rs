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

/// Completed tasks at or above this duration display elapsed time even outside `-v`.
const SLOW_TASK_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(2);

/// Rendering options for a single task row.
#[derive(Clone, Copy, Debug)]
pub(super) struct RowOpts {
    pub(super) mode: SummaryMode,
    pub(super) style: StyleChoice,
    pub(super) diagnostic_style: StyleChoice,
    pub(super) symbols: bool,
    pub(super) columns: Option<usize>,
    /// Verbose rows show every applicable task, per-task timing, and uncapped details.
    pub(super) verbose: bool,
    /// Quiet mode suppresses stdout and routes failure details to stderr.
    pub(super) quiet: bool,
}

/// A completed task's status and ordered details, ready for console output.
pub(super) struct TaskBlock {
    pub(super) status: Option<String>,
    pub(super) details: Vec<TaskDetail>,
    pub(super) expanded: bool,
}

pub(super) struct TaskDetail {
    pub(super) kind: MsgKind,
    pub(super) message: String,
    pub(super) line: String,
    pub(super) error_stream: bool,
}

pub(super) fn task_block(task: &TaskEntry, entries: &[LogEntry], opts: RowOpts) -> TaskBlock {
    let mut block = TaskBlock {
        status: None,
        details: Vec::new(),
        expanded: false,
    };
    if !task.visibility.is_visible() || task.status == TaskStatus::NotApplicable {
        return block;
    }
    let emit_row = should_emit_task_result(task, opts.verbose);
    let visible_entries: Vec<_> = entries
        .iter()
        .filter(|entry| detail_is_visible(entry, task, opts.verbose, emit_row))
        .map(|entry| (entry, entry.action_columns()))
        .collect();
    let verb_width = visible_entries
        .iter()
        .filter_map(|(_, columns)| columns.map(|(verb, _)| verb))
        .max()
        .unwrap_or(0);
    let mixed_verbs = visible_entries
        .iter()
        .filter_map(|(_, columns)| columns.map(|(verb, _)| verb))
        .any(|verb| verb != verb_width);

    for (entry, action_columns) in visible_entries {
        let diagnostic = matches!(
            entry.kind(),
            MsgKind::Warn | MsgKind::WarnContinuation | MsgKind::Error
        );
        let action_gap = action_columns.map_or(1, |(verb, line)| {
            if !mixed_verbs {
                return 1;
            }
            let gap = verb_width - verb + 2;
            match opts.columns {
                Some(columns) if line <= columns => gap.min(columns - line + 1),
                _ => gap,
            }
        });
        // Failed-task explanations remain stdout; warnings keep their severity.
        let explanatory_error =
            !opts.verbose && task.status == TaskStatus::Failed && entry.kind() == MsgKind::Error;
        let style = if diagnostic && !explanatory_error {
            opts.diagnostic_style
        } else {
            opts.style
        };
        let line = if task.status == TaskStatus::Ok && !emit_row {
            standalone_diagnostic(&task.name, entry, style, opts.symbols)
        } else if explanatory_error {
            Some(indented(entry.message(), style))
        } else {
            entry
                .console_line(style, true, opts.symbols, action_gap)
                .map(|line| {
                    if diagnostic {
                        format!("  {line}")
                    } else if !opts.verbose && matches!(entry, LogEntry::Message { .. }) {
                        indented(entry.message(), style)
                    } else {
                        line
                    }
                })
        };
        if let Some(line) = line.filter(|line| {
            !crate::infra::logging::utils::strip_ansi(line)
                .trim()
                .is_empty()
        }) {
            block.details.push(TaskDetail {
                kind: entry.kind(),
                message: entry.message().into(),
                line,
                error_stream: diagnostic && (!explanatory_error || opts.quiet),
            });
        }
    }
    block.expanded = !block.details.is_empty();
    if emit_row {
        let show_reason = opts.verbose
            || !block.details.iter().any(|detail| {
                !matches!(
                    detail.kind,
                    MsgKind::Warn | MsgKind::WarnContinuation | MsgKind::Error
                )
            })
            || matches!(
                task.status,
                TaskStatus::Failed
                    | TaskStatus::Skipped
                    | TaskStatus::Blocked
                    | TaskStatus::Interrupted
            );
        let mut row = format_task_line_with_reason(task, opts, show_reason);
        if block.expanded
            && let Some(badge) = action_badge(task)
        {
            row.push_str(&opts.style.paint(TextStyle::Dim, &badge));
        }
        block.status = Some(row);
    }
    block
}

fn standalone_diagnostic(
    name: &str,
    entry: &LogEntry,
    style: StyleChoice,
    symbols: bool,
) -> Option<String> {
    let message = if entry.kind() == MsgKind::WarnContinuation {
        format!("  {}", entry.message())
    } else {
        format!("{name}: {}", entry.message())
    };
    crate::infra::logging::console::ui_line_with_style(
        entry.kind(),
        &message,
        style,
        false,
        symbols,
    )
}

fn detail_is_visible(entry: &LogEntry, task: &TaskEntry, verbose: bool, emit_row: bool) -> bool {
    let message = task.message.as_deref();
    if verbose {
        return entry.verbose_detail(message).is_some();
    }
    if task.status == TaskStatus::Ok {
        // There is no status row to carry a warning, even when it matches the reason.
        return entry.is_visible_in_non_verbose(task.status, None);
    }
    if duplicates_task_message(entry.message(), message) {
        return false;
    }
    let normal_detail = emit_row
        && should_record_task_details(task.status)
        && entry.detail_line(task.status).is_some()
        && Some(entry.message()) != row_reason(task);
    normal_detail || entry.is_visible_in_non_verbose(task.status, message)
}

fn action_badge(task: &TaskEntry) -> Option<String> {
    match task.status {
        TaskStatus::Changed if task.actions.applied > 1 => {
            Some(format!(" · {} actions applied", task.actions.applied))
        }
        TaskStatus::DryRun if task.actions.planned > 1 => {
            Some(format!(" · {} actions", task.actions.planned))
        }
        TaskStatus::Changed
        | TaskStatus::DryRun
        | TaskStatus::Ok
        | TaskStatus::Passed
        | TaskStatus::NotApplicable
        | TaskStatus::Skipped
        | TaskStatus::Blocked
        | TaskStatus::Interrupted
        | TaskStatus::Failed => None,
    }
}

fn indented(text: &str, style: StyleChoice) -> String {
    text.lines()
        .map(|line| style.clean(&format!("  {}", line.trim_start())))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether a task produces a console row.
///
/// Non-verbose runs report only outcomes that need attention. Verbose runs also
/// report current tasks, but tasks that do not apply to this host stay out of
/// the console in both modes.
fn should_emit_task_result(task: &TaskEntry, verbose: bool) -> bool {
    if task.is_unstarted_interruption()
        || (task.result_display == TaskResultDisplay::RestartNotice
            && task.status == TaskStatus::Changed)
    {
        return false;
    }
    match task.status {
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
#[cfg(test)]
pub(super) fn format_task_line(task: &TaskEntry, opts: RowOpts) -> String {
    format_task_line_with_reason(task, opts, true)
}

fn format_task_line_with_reason(task: &TaskEntry, opts: RowOpts, show_reason: bool) -> String {
    let (label, text_style) = status::presentation(task.status, opts.mode, opts.symbols);
    let mut line = format!(
        "{} {}",
        opts.style.paint(text_style, label),
        if task.status == TaskStatus::Ok {
            opts.style.clean(&task.name)
        } else {
            opts.style.paint(TextStyle::Bold, &task.name)
        }
    );

    let annotation = status::annotation(task.status);
    if opts.symbols
        && let Some((word, color)) = annotation
    {
        line.push_str(&opts.style.paint(TextStyle::Dim, " · "));
        line.push_str(&opts.style.paint(color, word));
    }
    if show_reason && let Some(reason) = row_reason(task) {
        // The state is already named by the annotation or words-only label.
        if !annotation.is_some_and(|(word, _)| reason.eq_ignore_ascii_case(word)) {
            if matches!(
                task.status,
                TaskStatus::Failed
                    | TaskStatus::Skipped
                    | TaskStatus::Blocked
                    | TaskStatus::Interrupted
            ) {
                line.push_str(&opts.style.paint(TextStyle::Dim, " · "));
                line.push_str(&opts.style.clean(reason));
            } else {
                line.push_str(&opts.style.paint(TextStyle::Dim, &format!(" · {reason}")));
            }
        }
    }
    // A not-applicable task never ran, so its timing is either absent or a
    // meaningless `0.0s`. Suppressing it keeps `⁃` rows uniform whether the
    // task bailed out in `should_run()` or after being configured.
    if task.status != TaskStatus::NotApplicable
        && let Some(duration) = task.duration
        && (opts.verbose || duration >= SLOW_TASK_THRESHOLD)
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
