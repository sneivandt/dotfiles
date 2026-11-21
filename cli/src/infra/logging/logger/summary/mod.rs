//! End-of-run summary printing for [`Logger`].
//!
//! Renders final aggregate counts and compact completed-task rows.

use std::io::IsTerminal as _;

use super::Logger;
use crate::infra::logging::buffered::entry::LogEntry;
use crate::infra::logging::types::MsgKind;

mod render;
mod status;
mod totals;

use crate::infra::logging::utils::{format_elapsed, terminal_columns};
use render::{RowOpts, task_block};
use totals::{
    SummaryCounts, SummaryMode, format_summary_counts, should_space_before_totals, with_elapsed,
};

impl Logger {
    /// Print the summary of all recorded tasks.
    pub fn print_summary(&self, disposition: crate::infra::logging::CompletionDisposition) {
        let counts = SummaryCounts::from_tasks(&self.lock_tasks());
        self.clear_status();
        self.separate_from_startup();

        let elapsed = self.invocation_elapsed();
        let elapsed_str = format_elapsed(elapsed);

        let summary_mode = SummaryMode::for_command(&self.command);
        let style = self.console.stdout_style;

        if self.needs_totals_separator() {
            self.console.task_result("");
        }
        let counts = format_summary_counts(counts, summary_mode, style, disposition);
        let line = with_elapsed(&counts, &elapsed_str, style);
        if let Some(log) = &self.run_log {
            // RunFinish owns process timing. A restarted invocation also has a total wall time.
            log.emit(
                crate::infra::logging::LogEvent::Info,
                if self.elapsed_before_start.is_zero() {
                    &counts
                } else {
                    &line
                },
            );
        }
        self.emit_console(MsgKind::Always, &line);
    }

    /// Whether a blank line should separate the totals from preceding output.
    fn needs_totals_separator(&self) -> bool {
        let after_transition = self.console.lock().last_was_transition;
        after_transition
            || should_space_before_totals(&self.command, self.has_task_console_output())
    }

    /// Rendering options for task rows emitted by this logger.
    fn row_opts(&self) -> RowOpts {
        RowOpts {
            mode: SummaryMode::for_command(&self.command),
            style: if self.is_quiet() {
                self.console.stderr_style
            } else {
                self.console.stdout_style
            },
            diagnostic_style: self.console.stderr_style,
            symbols: self.symbols,
            columns: std::io::stdout().is_terminal().then(terminal_columns),
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
        if self.is_quiet() {
            use crate::infra::logging::TaskStatus;
            if let Some(row) = &block.status {
                match task.status {
                    TaskStatus::Failed => self.emit_console(MsgKind::Error, row),
                    TaskStatus::Skipped | TaskStatus::Blocked | TaskStatus::Interrupted => {
                        self.emit_console(MsgKind::Warn, row);
                    }
                    TaskStatus::Changed
                    | TaskStatus::Ok
                    | TaskStatus::Passed
                    | TaskStatus::NotApplicable
                    | TaskStatus::DryRun => {}
                }
            }
        }
        let visible = block.status.is_some()
            || (task.status == crate::infra::logging::TaskStatus::Ok && block.expanded);
        if visible {
            self.begin_task_block(block.expanded);
        }
        if let Some(row) = &block.status {
            self.console.task_result(row);
        }
        for detail in block.details {
            self.console.detail(
                detail.kind,
                &detail.message,
                &detail.line,
                detail.error_stream,
            );
        }
        if visible {
            self.end_task_block(block.expanded);
        }
    }

    /// Separate expanded blocks while keeping ordinary check rows compact.
    fn begin_task_block(&self, has_details: bool) {
        self.console.lock().begin_task_block(has_details);
    }

    fn end_task_block(&self, has_details: bool) {
        self.console.lock().end_task_block(has_details);
    }

    #[cfg(test)]
    fn should_separate_task_blocks(&self, has_details: bool) -> bool {
        self.console.lock().should_separate_task_blocks(has_details)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
