//! In-progress status line rendering for [`Logger`].

use std::io::IsTerminal as _;
use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

use super::Logger;
use crate::infra::logging::style::{StyleChoice, TextStyle};
use crate::infra::logging::utils::{strip_ansi, terminal_columns};

const PROGRESS_ELLIPSIS: &str = " …";

/// Return whether stdout is an interactive terminal that can handle redraws.
#[must_use]
pub(in crate::infra::logging) fn stdout_supports_progress() -> bool {
    std::io::stdout().is_terminal()
}

/// Fit a transient row to the terminal, then apply its presentation.
/// Applying styles last keeps ANSI escapes out of terminal-width calculations.
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

    style_transient_line(&display, style)
}

/// Emphasize the live state without muting the work the user is waiting on.
/// The remaining count is supporting context; active task names retain the
/// terminal's normal foreground contrast.
fn style_transient_line(line: &str, style: StyleChoice) -> String {
    let Some(rest) = line.strip_prefix("Running") else {
        return style.clean(line);
    };
    let running = style.paint(TextStyle::Bold, "Running");
    let Some(details) = rest.strip_prefix(" · ") else {
        return format!("{running}{}", style.clean(rest));
    };
    let Some((remaining, active)) = details.split_once(" · ") else {
        return format!("{running}{}", style.clean(rest));
    };
    if !remaining.ends_with(" remaining") {
        return format!("{running}{}", style.clean(rest));
    }

    format!(
        "{running}{}{}",
        style.paint(TextStyle::Dim, &format!(" · {remaining} · ")),
        style.clean(active)
    )
}

impl Logger {
    pub(in crate::infra::logging) fn clear_progress(&self) {
        self.console.lock().clear_progress();
    }

    pub(in crate::infra::logging) fn replace_status_line(&self, line: &str) {
        let line =
            transient_display_line_with_style(line, terminal_columns(), self.console.stdout_style);
        self.console.lock().replace_status(&line);
    }

    pub(in crate::infra::logging) fn draw_active_status(&self, line: &str) {
        let line =
            transient_display_line_with_style(line, terminal_columns(), self.console.stdout_style);
        self.console.lock().active_status(&line);
    }

    pub(in crate::infra::logging) fn has_task_console_output(&self) -> bool {
        self.console.lock().task_output_emitted
    }

    #[cfg(test)]
    pub(in crate::infra::logging) fn console_ends_with_blank_line(&self) -> bool {
        self.console.lock().ends_with_blank_line
    }

    #[cfg(test)]
    pub(in crate::infra::logging) fn mark_task_console_output(&self) {
        self.console.lock().task_output_emitted = true;
    }

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
    fn generic_transient_line_uses_normal_contrast_after_truncation() {
        assert_eq!(
            super::transient_display_line_with_style(
                "\x1b[32mabcdefghij\x1b[0m",
                8,
                StyleChoice::colored()
            ),
            "abcdef …"
        );
    }

    #[test]
    fn running_line_emphasizes_state_and_keeps_active_tasks_at_normal_contrast() {
        assert_eq!(
            super::transient_display_line_with_style(
                "Running · 4 remaining · Home symlinks, System packages",
                80,
                StyleChoice::colored(),
            ),
            "\x1b[1mRunning\x1b[0m\x1b[2m · 4 remaining · \x1b[0mHome symlinks, System packages"
        );
    }

    #[test]
    fn running_line_without_a_scheduled_count_keeps_task_name_at_normal_contrast() {
        assert_eq!(
            super::transient_display_line_with_style(
                "Running · task-a",
                80,
                StyleChoice::colored(),
            ),
            "\x1b[1mRunning\x1b[0m · task-a"
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
