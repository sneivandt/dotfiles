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
use render::{format_task_line, should_emit_task_result, task_result_lines};
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

        if should_space_before_totals(&self.command, self.verbose, counts.has_visible_tasks()) {
            self.task_result("");
        }
        for line in format_summary_lines(counts, summary_mode, self.dry_run, &elapsed_str, style) {
            self.always(&line);
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

        let lines = task_result_lines(
            &task,
            &details,
            SummaryMode::for_command(&self.command),
            stdout_style(),
        );
        if lines.is_empty() {
            return;
        }

        self.separate_from_startup();
        for line in lines {
            self.task_result(&line);
        }
        self.mark_task_console_output();
    }

    pub(in crate::infra::logging) fn emit_recorded_task_status(&self, task_name: &str) {
        let Some(task) = self.recorded_task(task_name) else {
            return;
        };
        if !should_emit_task_result(task.status) {
            return;
        }

        self.separate_from_startup();
        self.task_result(&format_task_line(
            &task,
            SummaryMode::for_command(&self.command),
            stdout_style(),
        ));
        self.mark_task_console_output();
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
