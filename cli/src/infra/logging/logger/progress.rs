//! In-progress status line rendering for [`Logger`].

use std::io::IsTerminal as _;
use std::io::Write as _;
use std::sync::atomic::Ordering;
use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

use super::Logger;
use crate::infra::logging::style::{StyleChoice, TextStyle, stdout_style};
use crate::infra::logging::subscriber;
use crate::infra::logging::utils::{strip_ansi, terminal_columns};

const PROGRESS_ELLIPSIS: &str = " …";

/// Return whether stdout is an interactive terminal that can handle redraws.
#[must_use]
pub(in crate::infra::logging) fn stdout_supports_progress() -> bool {
    std::io::stdout().is_terminal()
}

fn transient_display_line(line: &str, cols: usize) -> String {
    transient_display_line_with_style(line, cols, stdout_style())
}

/// Fit a transient row to the terminal, then dim the finished text in one pass.
/// Applying the style last keeps truncation from dropping the ANSI color.
fn transient_display_line_with_style(line: &str, cols: usize, style: StyleChoice) -> String {
    let plain = strip_ansi(line);
    let display = if plain.width() <= cols {
        plain
    } else {
        let ellipsis = if cols >= PROGRESS_ELLIPSIS.width() {
            PROGRESS_ELLIPSIS
        } else {
            PROGRESS_ELLIPSIS.trim()
        };
        let text_width = cols.saturating_sub(ellipsis.width());
        let truncated: String = plain
            .chars()
            .scan(0_usize, |width, ch| {
                *width = (*width).saturating_add(ch.width().unwrap_or(0));
                (*width <= text_width).then_some(ch)
            })
            .collect();
        format!("{truncated}{ellipsis}")
    };

    style.paint(TextStyle::Dim, &display)
}

#[allow(clippy::print_stdout, reason = "intentional user-facing output")]
impl Logger {
    /// Erase the transient status area from the console.
    ///
    /// No-op if no status rows are currently shown.
    /// Must be called while holding `flush_lock`.
    pub(in crate::infra::logging) fn clear_progress(&self) {
        let rows = subscriber::take_transient_progress_rows();
        if rows == 0 {
            self.progress_rows.store(0, Ordering::Relaxed);
            self.status_row_visible.store(false, Ordering::Relaxed);
            return;
        }

        print!("{}", subscriber::progress_clear_sequence(rows));
        drop(std::io::stdout().flush());
        self.progress_rows.store(0, Ordering::Relaxed);
        self.status_row_visible.store(false, Ordering::Relaxed);
    }

    /// Replace the currently displayed active-task row in place.
    ///
    /// Must be called while holding `flush_lock`.
    pub(in crate::infra::logging) fn replace_status_line(&self, line: &str) {
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
    pub(in crate::infra::logging) fn append_status_line(&self, line: &str, leading_blank: bool) {
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
    pub(in crate::infra::logging) fn set_status_row_visible(&self, visible: bool) {
        self.status_row_visible.store(visible, Ordering::Relaxed);
    }

    /// Return whether the current transient status area ends with an active-task row.
    pub(in crate::infra::logging) fn has_status_row(&self) -> bool {
        self.status_row_visible.load(Ordering::Relaxed)
    }

    /// Return whether any transient status rows are currently displayed.
    pub(in crate::infra::logging) fn has_transient_rows(&self) -> bool {
        self.progress_rows.load(Ordering::Relaxed) > 0
    }

    /// Return whether completed tasks have emitted durable console output.
    pub(in crate::infra::logging) fn has_task_console_output(&self) -> bool {
        self.task_console_output_emitted.load(Ordering::Relaxed)
    }

    /// Return whether the durable console output already ends with a blank line.
    pub(in crate::infra::logging) fn console_ends_with_blank_line(&self) -> bool {
        self.console_ends_with_blank_line.load(Ordering::Relaxed)
    }

    /// Remember that a completed task emitted durable console output.
    pub(in crate::infra::logging) fn mark_task_console_output(&self) {
        self.task_console_output_emitted
            .store(true, Ordering::Relaxed);
    }

    /// Clear any transient status rows from the console.
    pub(in crate::infra::logging) fn clear_status(&self) {
        let _guard = self.lock_flush();
        self.clear_progress();
    }
}

#[cfg(test)]
mod tests {
    use crate::infra::logging::isolated_logger;
    use crate::infra::logging::style::StyleChoice;

    #[test]
    fn progress_rows_zero_initially() {
        let (log, _tmp, _guard) = isolated_logger();
        assert_eq!(log.progress_rows_count(), 0);
    }

    #[test]
    fn truncated_transient_line_stays_dim() {
        assert_eq!(
            super::transient_display_line_with_style(
                "\x1b[32mabcdefghij\x1b[0m",
                8,
                StyleChoice::colored()
            ),
            "\x1b[2mabcdef …\x1b[0m"
        );
    }

    #[test]
    fn transient_line_truncates_by_terminal_cell_width() {
        assert_eq!(
            super::transient_display_line_with_style("界界界", 4, StyleChoice::plain()),
            "界 …"
        );
    }
}
