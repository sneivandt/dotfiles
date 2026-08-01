//! End-of-run summary printing for [`Logger`].
//!
//! Renders final aggregate counts and compact completed-task rows.

use super::Logger;
use crate::infra::logging::OutputExt as _;
use crate::infra::logging::style::stdout_style;
use crate::infra::logging::types::TaskEntry;

mod render;
mod totals;

use crate::infra::logging::utils::format_elapsed;
use render::{RowOpts, format_task_line, should_emit_task_result, task_result_lines};
use std::sync::atomic::Ordering;
use totals::{SummaryCounts, SummaryMode, format_summary_lines, should_space_before_totals};

impl Logger {
    /// Print the summary of all recorded tasks.
    pub fn print_summary(&self) {
        let tasks = match self.tasks.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        self.clear_status();

        let elapsed = self.start.elapsed();
        let elapsed_str = format_elapsed(elapsed);

        let summary_mode = SummaryMode::for_command(&self.command);
        let counts = SummaryCounts::from_tasks(&tasks);
        let style = stdout_style();

        if self.needs_totals_separator() {
            self.task_result("");
        }
        for line in format_summary_lines(counts, summary_mode, self.dry_run, &elapsed_str, style) {
            self.always(&line);
        }
    }

    /// Whether a blank line should separate the totals from preceding output.
    fn needs_totals_separator(&self) -> bool {
        should_space_before_totals(&self.command, self.has_task_console_output())
    }

    /// Rendering options for task rows emitted by this logger.
    fn row_opts(&self) -> RowOpts {
        RowOpts {
            mode: SummaryMode::for_command(&self.command),
            style: stdout_style(),
            verbose: self.verbose,
        }
    }

    pub(in crate::infra::logging) fn emit_recorded_task_result(&self, task_name: &str) {
        let task = self.recorded_task(task_name);
        let Some(task) = task else {
            return;
        };
        let details = self
            .task_details
            .lock()
            .map_or_else(|_| Vec::new(), |guard| guard.clone());

        let lines = task_result_lines(&task, &details, self.row_opts());
        let Some((status_row, detail_rows)) = lines.split_first() else {
            return;
        };

        self.begin_task_block();
        self.task_result(status_row);
        for line in detail_rows {
            self.task_result(line);
        }
        self.end_task_block(!detail_rows.is_empty());
    }

    pub(in crate::infra::logging) fn emit_recorded_task_status(&self, task_name: &str) {
        let Some(task) = self.recorded_task(task_name) else {
            return;
        };
        if !should_emit_task_result(task.status, self.verbose) {
            return;
        }

        self.begin_task_block();
        self.task_result(&format_task_line(&task, self.row_opts()));
        self.mark_task_console_output();
    }

    /// Open a task block, separating it from a preceding block of details.
    ///
    /// Without this a long list of actions runs straight into the next task's
    /// status row, and the two blocks read as one.
    fn begin_task_block(&self) {
        self.separate_from_startup();
        if self.last_block_had_details.swap(false, Ordering::Relaxed) {
            self.task_result("");
        }
    }

    /// Close a task block, remembering whether it printed indented details.
    fn end_task_block(&self, had_details: bool) {
        self.last_block_had_details
            .store(had_details, Ordering::Relaxed);
        self.mark_task_console_output();
    }

    /// Remember that verbose replay printed indented details for a task.
    pub(in crate::infra::logging) fn note_replayed_details(&self, had_details: bool) {
        if had_details {
            self.last_block_had_details.store(true, Ordering::Relaxed);
        }
    }

    fn recorded_task(&self, task_name: &str) -> Option<TaskEntry> {
        self.tasks.lock().map_or(None, |guard| {
            guard
                .iter()
                .rev()
                .find(|task| task.name == task_name)
                .cloned()
        })
    }
}

#[cfg(test)]
#[path = "tests.rs"]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests;
