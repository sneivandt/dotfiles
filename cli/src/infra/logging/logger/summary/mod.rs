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
        if let Some(path) = self.log_path() {
            self.startup(format!("Log: {}", path.display()));
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

    pub(in crate::infra::logging) fn emit_recorded_task_result(&self, task_name: &str) {
        let task = self.recorded_task(task_name);
        let Some(task) = task else {
            return;
        };
        let details = self.lock_task_details().clone();

        let lines = task_result_lines(&task, &details, self.row_opts());
        let Some((status_row, detail_rows)) = lines.split_first() else {
            return;
        };

        self.begin_task_block();
        self.task_result(status_row);
        for line in detail_rows {
            self.task_result(line);
        }
        self.end_task_block();
    }

    pub(in crate::infra::logging) fn emit_recorded_task_result_by_id(&self, task_id: &str) {
        let task = self.recorded_task_by_id(task_id);
        let Some(task) = task else {
            return;
        };
        let details = self.lock_task_details().clone();
        let lines = task_result_lines(&task, &details, self.row_opts());
        let Some((status_row, detail_rows)) = lines.split_first() else {
            return;
        };
        self.begin_task_block();
        self.task_result(status_row);
        for line in detail_rows {
            self.task_result(line);
        }
        self.end_task_block();
    }

    pub(in crate::infra::logging) fn emit_recorded_task_status(&self, task_name: &str) {
        let Some(task) = self.recorded_task(task_name) else {
            return;
        };
        if !task.visibility.is_visible() || !should_emit_task_result(task.status, self.verbose) {
            return;
        }
        self.begin_task_block();
        self.task_result(&format_task_line(&task, self.row_opts()));
        self.mark_task_console_output();
    }

    pub(in crate::infra::logging) fn emit_recorded_task_status_by_id(&self, task_id: &str) {
        let Some(task) = self.recorded_task_by_id(task_id) else {
            return;
        };
        if !task.visibility.is_visible() || !should_emit_task_result(task.status, self.verbose) {
            return;
        }
        self.begin_task_block();
        self.task_result(&format_task_line(&task, self.row_opts()));
        self.mark_task_console_output();
    }

    /// Open a task block after the startup separator.
    fn begin_task_block(&self) {
        self.separate_from_startup();
    }

    /// Close a task block and mark its output as durable.
    fn end_task_block(&self) {
        self.mark_task_console_output();
    }

    fn recorded_task(&self, task_name: &str) -> Option<TaskEntry> {
        self.lock_tasks()
            .iter()
            .rev()
            .find(|task| task.name == task_name)
            .cloned()
    }

    fn recorded_task_by_id(&self, task_id: &str) -> Option<TaskEntry> {
        self.lock_tasks()
            .iter()
            .rev()
            .find(|task| task.task_id.as_deref() == Some(task_id))
            .cloned()
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
