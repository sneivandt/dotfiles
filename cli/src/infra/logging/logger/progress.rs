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
        if cols == 0 {
            return String::new();
        }
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

    style.clean(&display)
}

/// Fit active names while reserving room for the overflow count.
fn active_names(active: &[String], limit: usize, cols: usize) -> (String, usize) {
    let shown = active.len().min(limit);
    for count in (1..=shown).rev() {
        let hidden = active.len() - count;
        let suffix = if hidden > 0 {
            format!(" +{hidden}")
        } else {
            String::new()
        };
        let names = active
            .iter()
            .take(count)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if names.width() + suffix.width() <= cols {
            return (names, hidden);
        }
    }
    let hidden = active.len().saturating_sub(1);
    let suffix_width = if hidden > 0 {
        format!(" +{hidden}").width()
    } else {
        0
    };
    let names = transient_display_line_with_style(
        active.first().map_or("", String::as_str),
        cols.saturating_sub(suffix_width),
        StyleChoice::auto(false, true),
    );
    (names, hidden)
}

fn active_display_line(
    active: &[String],
    remaining: Option<usize>,
    frame: usize,
    symbols: bool,
    verbose: bool,
    cols: usize,
    style: StyleChoice,
) -> String {
    let active: Vec<String> = active.iter().map(|name| strip_ansi(name)).collect();
    let spinner = if symbols {
        ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
            .get(frame % 10)
            .copied()
            .unwrap_or("⠋")
    } else {
        ["|", "/", "-", "\\"].get(frame % 4).copied().unwrap_or("|")
    };
    let count = remaining.map_or_else(String::new, |count| {
        let unit = if count == 1 { "task" } else { "tasks" };
        format!("{count} {unit} left · ")
    });
    let prefix = format!("{spinner} {count}");
    // Very narrow terminals still get a bounded line, even without room for names.
    if prefix.width() >= cols || active.is_empty() {
        return transient_display_line_with_style(&prefix, cols, style);
    }
    let (names, hidden) = active_names(
        &active,
        if verbose { active.len() } else { 3 },
        cols - prefix.width(),
    );
    let suffix = if hidden > 0 {
        format!(" +{hidden}")
    } else {
        String::new()
    };
    let line = format!("{prefix}{names}{suffix}");
    if line.width() > cols {
        return transient_display_line_with_style(&line, cols, style);
    }
    format!(
        "{} {}{}{}",
        style.paint(TextStyle::MutedCyan, spinner),
        style.paint(TextStyle::Dim, &count),
        names,
        style.paint(TextStyle::Dim, &suffix)
    )
}

/// Keeps animation scoped to the command and wakes immediately on shutdown.
struct ProgressTicker {
    stop: std::sync::mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for ProgressTicker {
    fn drop(&mut self) {
        let _stop_sent = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
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

    pub(in crate::infra::logging) fn draw_active_status(&self, active: &[String]) {
        let remaining = self
            .task_progress()
            .map(|(done, total)| total.saturating_sub(done));
        let line = active_display_line(
            active,
            remaining,
            self.progress_frame
                .load(std::sync::atomic::Ordering::Relaxed),
            self.symbols,
            self.is_verbose(),
            terminal_columns(),
            self.console.stdout_style,
        );
        self.console.lock().active_status(&line);
    }

    /// Animate only the existing task footer; never revive cleared or custom status lines.
    fn tick_progress(&self) {
        let _guard = self.lock_flush();
        if !self.console.lock().active_progress {
            return;
        }
        self.progress_frame
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.redraw_active_status_locked(true);
    }

    pub(crate) fn start_progress(self: &std::sync::Arc<Self>) -> impl Drop {
        self.start_progress_with_terminal(stdout_supports_progress())
    }

    fn start_progress_with_terminal(self: &std::sync::Arc<Self>, terminal: bool) -> ProgressTicker {
        let (stop, receiver) = std::sync::mpsc::channel();
        let weak = std::sync::Arc::downgrade(self);
        let thread =
            if terminal && self.console.lock().mode == crate::infra::logging::OutputMode::Human {
                std::thread::Builder::new()
                    .name("cli-progress".into())
                    .spawn(move || {
                        while matches!(
                            receiver.recv_timeout(std::time::Duration::from_millis(100)),
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                        ) {
                            let Some(log) = weak.upgrade() else {
                                break;
                            };
                            log.tick_progress();
                        }
                    })
                    .ok()
            } else {
                None
            };
        ProgressTicker { stop, thread }
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

    fn render(names: &[&str], remaining: Option<usize>, cols: usize, verbose: bool) -> String {
        let active = names
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>();
        super::active_display_line(
            &active,
            remaining,
            0,
            true,
            verbose,
            cols,
            StyleChoice::plain(),
        )
    }

    #[test]
    fn footer_counts_tasks_and_limits_names_without_limiting_verbose_output() {
        let names = ["Packages", "Symlinks", "Fonts", "Git", "Shell"];
        assert_eq!(
            render(&names, Some(12), 80, false),
            "⠋ 12 tasks left · Packages, Symlinks, Fonts +2"
        );
        assert_eq!(
            render(&names, Some(12), 80, true),
            "⠋ 12 tasks left · Packages, Symlinks, Fonts, Git, Shell"
        );
        assert_eq!(
            render(&["Packages"], Some(1), 80, false),
            "⠋ 1 task left · Packages"
        );
        assert_eq!(render(&["Packages"], None, 80, false), "⠋ Packages");
        assert_eq!(
            render(&["Packages"], Some(0), 80, false),
            "⠋ 0 tasks left · Packages"
        );
    }

    #[test]
    fn footer_styles_only_spinner_count_and_overflow() {
        let active = ["Packages", "Symlinks", "Fonts", "Git"].map(String::from);
        assert_eq!(
            super::active_display_line(
                &active,
                Some(4),
                0,
                true,
                false,
                80,
                StyleChoice::colored()
            ),
            "\x1b[2;36m⠋\x1b[0m \x1b[2m4 tasks left · \x1b[0mPackages, Symlinks, Fonts\x1b[2m +1\x1b[0m"
        );
    }

    #[test]
    fn exact_width_names_do_not_reserve_space_for_a_zero_overflow_count() {
        let names = ["Packages", "Symlinks", "Fonts"];
        for count in 1..=names.len() {
            let shown = names.get(..count).unwrap();
            let expected = format!("⠋ {}", shown.join(", "));
            let cols = unicode_width::UnicodeWidthStr::width(expected.as_str());
            for verbose in [false, true] {
                assert_eq!(render(shown, None, cols, verbose), expected);
            }
        }
        assert_eq!(
            render(&["Long package task"], None, 14, false),
            "⠋ Long packa …"
        );
    }

    #[test]
    fn exact_width_footer_retains_its_styling() {
        let names = ["Packages", "Symlinks", "Fonts", "Git"].map(String::from);
        let plain =
            super::active_display_line(&names, Some(4), 0, true, false, 80, StyleChoice::plain());
        let exact_width = unicode_width::UnicodeWidthStr::width(plain.as_str());
        assert_eq!(
            super::active_display_line(
                &names,
                Some(4),
                0,
                true,
                false,
                exact_width,
                StyleChoice::colored()
            ),
            super::active_display_line(&names, Some(4), 0, true, false, 80, StyleChoice::colored()),
            "a footer that fits exactly must keep its colors and dim context"
        );
    }

    #[test]
    fn narrow_footer_preserves_overflow_and_respects_unicode_cell_width() {
        assert_eq!(
            render(&["Long package task", "Fonts", "Git"], None, 14, false),
            "⠋ Long pa … +2"
        );
        assert_eq!(
            render(&["界界界界", "Fonts"], None, 11, false),
            "⠋ 界界 … +1"
        );
        for cols in 0..60 {
            for verbose in [false, true] {
                let line = render(
                    &["界界界界", "Fonts", "Git", "Shell"],
                    Some(12),
                    cols,
                    verbose,
                );
                assert!(
                    unicode_width::UnicodeWidthStr::width(line.as_str()) <= cols,
                    "width {cols}: {line}"
                );
            }
        }
    }

    #[test]
    fn spinner_frames_advance_with_an_ascii_fallback() {
        let names = ["Packages".to_string()];
        for (symbols, frames) in [
            (true, vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
            (false, vec!["|", "/", "-", "\\"]),
        ] {
            for frame in 0..frames.len() * 2 {
                assert_eq!(
                    super::active_display_line(
                        &names,
                        None,
                        frame,
                        symbols,
                        false,
                        80,
                        StyleChoice::plain()
                    ),
                    format!("{} Packages", frames[frame % frames.len()])
                );
            }
        }
    }

    #[test]
    fn footer_reads_remaining_count_from_task_accounting() {
        use crate::infra::logging::{
            TaskEntry, TaskStatus,
            types::{ActionCounts, TaskVisibility},
        };
        let (log, _tmp, _guard) = isolated_logger();
        log.add_task_total(2);
        log.notify_task_start_with_progress("Packages", true);
        assert!(
            log.console
                .lock()
                .captured
                .last()
                .unwrap()
                .1
                .contains("2 tasks left")
        );
        log.record_task(TaskEntry::new(
            "done",
            "Done",
            TaskStatus::Ok,
            None,
            ActionCounts::default(),
            TaskVisibility::Visible,
        ));
        log.mark_task_completed("done");
        log.tick_progress();
        assert!(
            log.console
                .lock()
                .captured
                .last()
                .unwrap()
                .1
                .contains("1 task left")
        );
    }

    #[test]
    fn animation_does_not_revive_cleared_or_custom_status_or_enter_logs() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start_with_progress("Packages", true);
        log.tick_progress();
        assert!(
            log.console
                .lock()
                .captured
                .last()
                .unwrap()
                .1
                .contains("⠙ Packages")
        );
        log.clear_status();
        let writes = log.console.lock().captured.len();
        log.tick_progress();
        assert_eq!(log.console.lock().captured.len(), writes);
        log.replace_status_line("Downloading release");
        let custom_writes = log.console.lock().captured.len();
        log.tick_progress();
        assert_eq!(log.console.lock().captured.len(), custom_writes);
        log.notify_task_done_with_progress("Packages", true);
        let completed_writes = log.console.lock().captured.len();
        log.tick_progress();
        assert_eq!(log.console.lock().captured.len(), completed_writes);
        let stored = std::fs::read_to_string(log.log_path().unwrap()).unwrap();
        assert!(!stored.contains("⠋") && !stored.contains("⠙"));
    }

    #[test]
    fn animation_shutdown_signals_and_joins_the_worker() {
        let (stop, receiver) = std::sync::mpsc::channel();
        let (completed, completion) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            completed.send(receiver.recv().is_ok()).unwrap();
        });
        let progress = super::ProgressTicker {
            stop,
            thread: Some(worker),
        };
        drop(progress);
        assert_eq!(
            completion.try_recv(),
            Ok(true),
            "shutdown must signal and join before returning"
        );
    }

    #[test]
    fn durable_output_suspends_animation_until_the_footer_is_redrawn() {
        use crate::infra::logging::OutputExt as _;
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start_with_progress("Packages", true);
        log.always("durable output");
        let writes = log.console.lock().captured.len();
        log.tick_progress();
        assert_eq!(log.console.lock().captured.len(), writes);
        log.notify_task_start_with_progress("Fonts", true);
        log.tick_progress();
        assert!(
            log.console
                .lock()
                .captured
                .last()
                .unwrap()
                .1
                .contains("⠙ Packages, Fonts")
        );
    }

    #[test]
    fn animation_worker_starts_only_for_a_human_terminal_and_releases_the_logger() {
        use crate::infra::logging::OutputMode;
        for mode in [OutputMode::Human, OutputMode::Quiet, OutputMode::Jsonl] {
            for terminal in [false, true] {
                let (mut log, _tmp, _guard) = isolated_logger();
                log.set_output_mode(mode);
                let log = std::sync::Arc::new(log);
                let weak = std::sync::Arc::downgrade(&log);
                let progress = log.start_progress_with_terminal(terminal);
                assert_eq!(
                    progress.thread.is_some(),
                    terminal && mode == OutputMode::Human,
                    "terminal={terminal}, mode={mode:?}"
                );
                drop(progress);
                drop(log);
                assert!(
                    weak.upgrade().is_none(),
                    "animation must not retain the run logger"
                );
            }
        }
    }

    #[test]
    fn non_human_modes_never_animate() {
        for mode in [
            crate::infra::logging::OutputMode::Quiet,
            crate::infra::logging::OutputMode::Jsonl,
        ] {
            let (mut log, _tmp, _guard) = isolated_logger();
            log.set_output_mode(mode);
            log.notify_task_start_with_progress("Packages", true);
            log.tick_progress();
            assert!(log.console.lock().captured.is_empty());
        }
    }

    #[test]
    fn transient_line_truncates_by_terminal_cell_width() {
        assert_eq!(
            super::transient_display_line_with_style("界界界", 4, StyleChoice::plain()),
            "界 …"
        );
    }
}
