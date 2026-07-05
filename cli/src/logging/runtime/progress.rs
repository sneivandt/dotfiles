//! In-progress status line rendering for [`Logger`].

use std::io::IsTerminal as _;
use std::io::Write as _;
use std::sync::atomic::Ordering;

use super::Logger;
use crate::logging::utils::{strip_ansi, terminal_columns};

const PROGRESS_ELLIPSIS: &str = " …";

/// Return whether stdout is an interactive terminal that can handle redraws.
#[must_use]
pub(in crate::logging) fn stdout_supports_progress() -> bool {
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
    pub(in crate::logging) fn clear_progress(&self) {
        let rows = self.progress_rows.load(Ordering::Relaxed);
        if rows == 0 {
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

    /// Print transient status rows to the console and mark them as shown.
    ///
    /// Must be called while holding `flush_lock`.
    pub(in crate::logging) fn draw_status_lines(&self, lines: &[String]) {
        if lines.is_empty() {
            return;
        }

        let cols = terminal_columns();
        for (idx, line) in lines.iter().enumerate() {
            if idx > 0 {
                println!();
            }
            print!("{}", transient_display_line(line, cols));
        }
        drop(std::io::stdout().flush());
        self.progress_rows.store(
            u16::try_from(lines.len()).unwrap_or(u16::MAX),
            Ordering::Relaxed,
        );
    }

    /// Replace the currently displayed active-task row in place.
    ///
    /// Must be called while holding `flush_lock`.
    pub(in crate::logging) fn replace_status_line(&self, line: &str) {
        print!(
            "\r\x1b[K{}",
            transient_display_line(line, terminal_columns())
        );
        drop(std::io::stdout().flush());
        self.set_status_row_visible(true);
    }

    /// Append an active-task row below the existing transient details.
    ///
    /// Must be called while holding `flush_lock`.
    pub(in crate::logging) fn append_status_line(&self, line: &str, leading_blank: bool) {
        let rows = self.progress_rows.load(Ordering::Relaxed);
        let added_rows = if rows == 0 {
            1
        } else if leading_blank {
            print!("\n\n");
            2
        } else {
            println!();
            1
        };
        print!("{}", transient_display_line(line, terminal_columns()));
        drop(std::io::stdout().flush());
        self.progress_rows
            .store(rows.saturating_add(added_rows), Ordering::Relaxed);
        self.set_status_row_visible(true);
    }

    /// Mark whether the current transient status area ends with an active-task row.
    pub(in crate::logging) fn set_status_row_visible(&self, visible: bool) {
        self.status_row_visible.store(visible, Ordering::Relaxed);
    }

    /// Return whether the current transient status area ends with an active-task row.
    pub(in crate::logging) fn has_status_row(&self) -> bool {
        self.status_row_visible.load(Ordering::Relaxed)
    }

    /// Return whether any transient status rows are currently displayed.
    pub(in crate::logging) fn has_transient_rows(&self) -> bool {
        self.progress_rows.load(Ordering::Relaxed) > 0
    }

    /// Clear any transient status rows from the console.
    pub(in crate::logging) fn clear_status(&self) {
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
    use crate::logging::isolated_logger;

    #[test]
    fn progress_rows_zero_initially() {
        let (log, _tmp, _guard) = isolated_logger();
        assert_eq!(log.progress_rows_count(), 0);
    }

    #[test]
    fn draw_status_lines_records_rendered_row_count() {
        let (log, _tmp, _guard) = isolated_logger();
        let long_names = "a".repeat(500);
        log.draw_status_lines(&[long_names]);
        assert_eq!(
            log.progress_rows_count(),
            1,
            "a single rendered status line should record one row"
        );
    }

    #[test]
    fn transient_display_line_strips_ansi_before_truncating() {
        assert_eq!(
            super::transient_display_line("\x1b[32mabcdefghij\x1b[0m", 8),
            "abcdef …"
        );
    }
}
