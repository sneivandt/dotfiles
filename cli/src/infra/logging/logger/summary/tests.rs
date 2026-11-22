use std::fmt::Write as _;
use std::time::Duration;

use super::render::{RowOpts, format_task_line, task_block};
use super::totals::{SummaryCounts, SummaryMode, should_space_before_totals};
use crate::infra::logging::CompletionDisposition;
use crate::infra::logging::buffered::entry::LogEntry;
use crate::infra::logging::style::StyleChoice;
use crate::infra::logging::types::{ActionCounts, MsgKind, TaskEntry, TaskStatus, TaskVisibility};
use crate::infra::logging::utils::format_elapsed;
use crate::infra::logging::{Logger, OutputExt as _};

/// Build a task entry with the fields a row-rendering test cares about.
fn task_entry(name: &str, status: TaskStatus, message: Option<&str>) -> TaskEntry {
    TaskEntry::new(
        name,
        name,
        status,
        message,
        ActionCounts::default(),
        TaskVisibility::Visible,
    )
}

fn record_task(log: &Logger, name: &str, status: TaskStatus, message: Option<&str>) {
    log.record_task(task_entry(name, status, message));
}

fn task_result_lines(task: &TaskEntry, details: &[String], opts: RowOpts) -> Vec<String> {
    let entries: Vec<_> = details
        .iter()
        .map(|message| LogEntry::Message {
            kind: MsgKind::Info,
            msg: message.clone(),
        })
        .collect();
    let block = task_block(task, &entries, opts);
    block
        .status
        .into_iter()
        .chain(
            block
                .details
                .into_iter()
                .flat_map(|detail| detail.line.lines().map(str::to_string).collect::<Vec<_>>()),
        )
        .collect()
}

/// Standard-mode, non-verbose row options.
fn plain_opts() -> RowOpts {
    RowOpts {
        mode: SummaryMode::Standard,
        style: StyleChoice::plain(),
        diagnostic_style: StyleChoice::plain(),
        symbols: true,
        columns: None,
        verbose: false,
    }
}

/// Standard-mode, non-verbose row options with colour enabled.
fn colored_opts() -> RowOpts {
    RowOpts {
        style: StyleChoice::colored(),
        diagnostic_style: StyleChoice::colored(),
        ..plain_opts()
    }
}

fn assert_hidden_row(task: &TaskEntry) {
    for verbose in [false, true] {
        let opts = RowOpts {
            verbose,
            ..plain_opts()
        };
        assert!(
            task_result_lines(task, &[], opts).is_empty(),
            "{task:?}, verbose={verbose}"
        );
    }
}

// Each totals fixture specifies just the counters it exercises.
macro_rules! counts {
    ($($field:ident: $value:expr),* $(,)?) => {
        SummaryCounts { $($field: $value,)* ..Default::default() }
    };
}

/// Count/vocabulary fixtures use successful completion; disposition is tested separately.
fn format_summary_lines(
    counts: SummaryCounts,
    mode: SummaryMode,
    elapsed: &str,
    style: StyleChoice,
) -> Vec<String> {
    summary_lines_with_disposition(
        counts,
        mode,
        elapsed,
        style,
        CompletionDisposition::Succeeded,
    )
}

fn summary_lines_with_disposition(
    counts: SummaryCounts,
    mode: SummaryMode,
    elapsed: &str,
    style: StyleChoice,
    disposition: CompletionDisposition,
) -> Vec<String> {
    let counts = super::totals::format_summary_counts(counts, mode, style, disposition);
    vec![super::totals::with_elapsed(&counts, elapsed, style)]
}

mod presentation;
mod rows;
mod totals;
