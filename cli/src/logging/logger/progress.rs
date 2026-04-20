//! In-progress status line rendering for [`Logger`].

use std::io::Write as _;
use std::sync::atomic::Ordering;

use super::Logger;
use crate::logging::utils::terminal_columns;

#[allow(clippy::print_stdout)]
impl Logger {
    /// Erase the in-progress status line from the console.
    ///
    /// No-op if no progress line is currently shown.
    /// Must be called while holding `flush_lock`.
    pub(in crate::logging) fn clear_progress(&self) {
        if self.progress_rows.load(Ordering::Relaxed) > 0 {
            print!("\r\x1b[K");
            std::io::stdout().flush().ok();
            self.progress_rows.store(0, Ordering::Relaxed);
        }
    }

    /// Print an in-progress status line to the console and mark it as shown.
    ///
    /// The task-name list is truncated to fit within a single terminal row so
    /// that [`clear_progress`](Self::clear_progress) never needs cursor-up
    /// movement (which is fragile when the terminal width is unknown).
    ///
    /// Must be called while holding `flush_lock`.
    pub(in crate::logging) fn draw_progress(&self, names: &str) {
        let cols = terminal_columns();
        let prefix_width = 4;
        let max_name_chars = cols.saturating_sub(prefix_width);
        let display_names = if names.chars().count() > max_name_chars {
            let truncated: String = names
                .chars()
                .take(max_name_chars.saturating_sub(1))
                .collect();
            format!("{truncated}…")
        } else {
            names.to_string()
        };
        print!("  \x1b[2m▹ {display_names}\x1b[0m");
        std::io::stdout().flush().ok();
        self.progress_rows.store(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use crate::logging::isolated_logger;

    #[test]
    fn progress_rows_zero_initially() {
        let (log, _tmp, _guard) = isolated_logger();
        assert_eq!(log.progress_rows_count(), 0);
    }

    #[test]
    fn draw_progress_caps_rows_to_one() {
        let (log, _tmp, _guard) = isolated_logger();
        let long_names = "a".repeat(500);
        log.draw_progress(&long_names);
        assert_eq!(
            log.progress_rows_count(),
            1,
            "progress_rows should always be 1 even for very long names"
        );
    }
}
