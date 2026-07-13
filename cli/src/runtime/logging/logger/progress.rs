//! In-progress status line rendering for [`Logger`].

use std::io::IsTerminal as _;
use std::io::Write as _;
use std::sync::atomic::Ordering;

use super::Logger;
use crate::runtime::logging::subscriber;
use crate::runtime::logging::utils::{strip_ansi, terminal_columns};

const PROGRESS_ELLIPSIS: &str = " …";

/// Return whether stdout is an interactive terminal that can handle redraws.
#[must_use]
pub(in crate::runtime::logging) fn stdout_supports_progress() -> bool {
    std::io::stdout().is_terminal()
}

fn transient_display_line(line: &str, cols: usize) -> String {
    let plain = strip_ansi(line);
    if plain.chars().count() <= cols {
        return line.to_string();
    }

    let ellipsis_width = PROGRESS_ELLIPSIS.chars().count();
    let truncated: String = plain
        .chars()
        .take(cols.saturating_sub(ellipsis_width))
        .collect();
    format!("{truncated}{PROGRESS_ELLIPSIS}")
}

#[allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "intentional user-facing output"
)]
impl Logger {
    /// Erase the transient status area from the console.
    ///
    /// No-op if no status rows are currently shown.
    /// Must be called while holding `flush_lock`.
    pub(in crate::runtime::logging) fn clear_progress(&self) {
        let rows = subscriber::take_transient_progress_rows();
        if rows == 0 {
            self.progress_rows.store(0, Ordering::Relaxed);
            self.status_row_visible.store(false, Ordering::Relaxed);
            return;
        }

        print!("\r\x1b[K");
        for _ in 1..usize::from(rows) {
            print!("\x1b[1A\r\x1b[K");
        }
        drop(std::io::stdout().flush());
        self.progress_rows.store(0, Ordering::Relaxed);
        self.status_row_visible.store(false, Ordering::Relaxed);
    }

    /// Replace the currently displayed active-task row in place.
    ///
    /// Must be called while holding `flush_lock`.
    pub(in crate::runtime::logging) fn replace_status_line(&self, line: &str) {
        let rows = if subscriber::transient_progress_rows() == 0 {
            1
        } else {
            self.progress_rows.load(Ordering::Relaxed).max(1)
        };
        print!(
            "\r\x1b[K{}",
            transient_display_line(line, terminal_columns())
        );
        drop(std::io::stdout().flush());
        self.progress_rows.store(rows, Ordering::Relaxed);
        subscriber::set_transient_progress(rows);
        self.set_status_row_visible(true);
    }

    /// Append an active-task row below the existing transient details.
    ///
    /// Must be called while holding `flush_lock`.
    pub(in crate::runtime::logging) fn append_status_line(&self, line: &str, leading_blank: bool) {
        let rows = self.progress_rows.load(Ordering::Relaxed);
        let added_rows = if rows == 0 {
            if leading_blank {
                println!();
                2
            } else {
                1
            }
        } else if leading_blank {
            print!("\n\n");
            2
        } else {
            println!();
            1
        };
        print!("{}", transient_display_line(line, terminal_columns()));
        drop(std::io::stdout().flush());
        let rows = rows.saturating_add(added_rows);
        self.progress_rows.store(rows, Ordering::Relaxed);
        subscriber::set_transient_progress(rows);
        self.set_status_row_visible(true);
    }

    /// Mark whether the current transient status area ends with an active-task row.
    pub(in crate::runtime::logging) fn set_status_row_visible(&self, visible: bool) {
        self.status_row_visible.store(visible, Ordering::Relaxed);
    }

    /// Return whether the current transient status area ends with an active-task row.
    pub(in crate::runtime::logging) fn has_status_row(&self) -> bool {
        self.status_row_visible.load(Ordering::Relaxed)
    }

    /// Return whether any transient status rows are currently displayed.
    pub(in crate::runtime::logging) fn has_transient_rows(&self) -> bool {
        self.progress_rows.load(Ordering::Relaxed) > 0
    }

    /// Return whether completed tasks have emitted durable console output.
    pub(in crate::runtime::logging) fn has_task_console_output(&self) -> bool {
        self.task_console_output_emitted.load(Ordering::Relaxed)
    }

    /// Remember that a completed task emitted durable console output.
    pub(in crate::runtime::logging) fn mark_task_console_output(&self) {
        self.task_console_output_emitted
            .store(true, Ordering::Relaxed);
    }

    /// Clear any transient status rows from the console.
    pub(in crate::runtime::logging) fn clear_status(&self) {
        let _guard = self.flush_lock.lock().unwrap_or_else(|e| {
            eprintln!("warning: flush lock was poisoned, recovering");
            e.into_inner()
        });
        self.clear_progress();
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use crate::runtime::logging::isolated_logger;

    #[test]
    fn progress_rows_zero_initially() {
        let (log, _tmp, _guard) = isolated_logger();
        assert_eq!(log.progress_rows_count(), 0);
    }

    #[test]
    fn transient_display_line_strips_ansi_before_truncating() {
        assert_eq!(
            super::transient_display_line("\x1b[32mabcdefghij\x1b[0m", 8),
            "abcdef …"
        );
    }
}
