//! End-of-run summary printing for [`Logger`].
//!
//! Renders final aggregate counts and compact completed-task rows.

use super::Logger;
use crate::infra::logging::OutputExt as _;
use crate::infra::logging::buffered::entry::LogEntry;

mod render;
mod status;
mod totals;

use crate::infra::logging::utils::format_elapsed;
use render::{RowOpts, task_block};
use totals::{SummaryCounts, SummaryMode, format_summary_lines, should_space_before_totals};

impl Logger {
    /// Print the summary of all recorded tasks.
    pub fn print_summary(&self) {
        let tasks = self.lock_tasks().clone();
        self.clear_status();
        self.separate_from_startup();

        let elapsed = self.start.elapsed();
        let elapsed_str = format_elapsed(elapsed);

        let summary_mode = SummaryMode::for_command(&self.command);
        let counts = SummaryCounts::from_tasks(&tasks);
        let style = self.console.stdout_style;

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
            style: self.console.stdout_style,
            symbols: self.symbols,
            verbose: self.is_verbose(),
        }
    }

    pub(in crate::infra::logging) fn emit_recorded_task_result(
        &self,
        task_id: &str,
        entries: &[LogEntry],
    ) {
        let task = self
            .lock_tasks()
            .iter()
            .rev()
            .find(|task| task.task_id == task_id)
            .cloned();
        let Some(task) = task else {
            return;
        };
        let block = task_block(&task, entries, self.row_opts());
        if let Some(row) = &block.status {
            self.begin_task_block(block.expanded);
            self.task_result(row);
        }
        for (kind, line) in block.details {
            self.emit_console(kind, &line);
        }
        if block.status.is_some() {
            self.end_task_block(block.expanded);
        }
    }

    /// Separate expanded blocks while keeping ordinary check rows compact.
    fn begin_task_block(&self, has_details: bool) {
        let spaced =
            SummaryMode::for_command(&self.command) == SummaryMode::Standard || self.is_verbose();
        self.console.lock().begin_task_block(has_details, spaced);
    }

    fn end_task_block(&self, has_details: bool) {
        self.console.lock().end_task_block(has_details);
    }

    #[cfg(test)]
    fn should_separate_task_blocks(&self, has_details: bool) -> bool {
        let spaced =
            SummaryMode::for_command(&self.command) == SummaryMode::Standard || self.is_verbose();
        self.console
            .lock()
            .should_separate_task_blocks(has_details, spaced)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
