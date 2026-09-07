//! End-of-run summary printing for [`Logger`].
//!
//! Renders final aggregate counts and compact completed-task rows.

use super::Logger;
use crate::infra::logging::OutputExt as _;
use crate::infra::logging::style::stdout_style;
use crate::infra::logging::types::TaskEntry;

mod render;
mod status;
mod totals;

use crate::infra::logging::utils::format_elapsed;
use render::{RowOpts, format_task_line, should_emit_task_result, task_result_lines};
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
            symbols: self.symbols,
            verbose: self.verbose,
        }
    }

    pub(in crate::infra::logging) fn emit_recorded_task_result(
        &self,
        task_id: &str,
        has_followup_rows: bool,
    ) {
        let task = self.recorded_task(task_id);
        let Some(task) = task else {
            return;
        };
        let details = self.lock_task_details().clone();

        let lines = task_result_lines(&task, &details, self.row_opts());
        let Some((status_row, detail_rows)) = lines.split_first() else {
            return;
        };

        let has_details = has_followup_rows || !detail_rows.is_empty();
        self.begin_task_block(has_details);
        self.task_result(status_row);
        for line in detail_rows {
            self.task_result(line);
        }
        self.end_task_block(has_details);
    }

    pub(in crate::infra::logging) fn emit_recorded_task_status(&self, task_id: &str) {
        let Some(task) = self.recorded_task(task_id) else {
            return;
        };
        if !task.visibility.is_visible() || !should_emit_task_result(task.status, self.verbose) {
            return;
        }
        self.begin_task_block(true);
        self.task_result(&format_task_line(&task, self.row_opts()));
        self.end_task_block(true);
    }

    /// Separate expanded task blocks while keeping ordinary check rows compact.
    fn begin_task_block(&self, has_details: bool) {
        self.separate_from_startup();
        if self.should_separate_task_blocks(has_details) {
            self.task_result("");
        }
    }

    /// Close a task block and mark its output as durable.
    fn end_task_block(&self, has_details: bool) {
        self.last_task_block_had_details
            .store(has_details, std::sync::atomic::Ordering::Relaxed);
        self.mark_task_console_output();
    }

    /// Decide whether the next task block needs a leading blank line.
    fn should_separate_task_blocks(&self, current_has_details: bool) -> bool {
        self.has_task_console_output()
            && (SummaryMode::for_command(&self.command) == SummaryMode::Standard
                || self.verbose
                || self
                    .last_task_block_had_details
                    .load(std::sync::atomic::Ordering::Relaxed)
                || current_has_details)
    }

    fn recorded_task(&self, task_id: &str) -> Option<TaskEntry> {
        self.lock_tasks()
            .iter()
            .rev()
            .find(|task| task.task_id == task_id)
            .cloned()
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
