//! Shared console writer and state for logger messages and raw tracing events.
use std::io::Write as _;
use std::sync::{Mutex, MutexGuard};

use super::style::{StyleChoice, TextStyle, stderr_style, stdout_style};
use super::types::MsgKind;
use super::utils::strip_ansi;

#[derive(Debug)]
pub(super) struct Console {
    state: Mutex<ConsoleState>,
    pub(super) stdout_style: StyleChoice,
    stderr_style: StyleChoice,
}

/// All terminal state changes and writes happen under the same lock.
#[derive(Debug, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "verbosity, output destination, and spacing are independent console facts"
)]
pub(super) struct ConsoleState {
    enabled: bool,
    pub(super) verbose: bool,
    pub(super) progress_rows: u16,
    pub(super) task_output_emitted: bool,
    pub(super) last_task_block_had_details: bool,
    pub(super) ends_with_blank_line: bool,
    pub(super) startup_separator_emitted: bool,
    #[cfg(test)]
    pub(super) captured: Vec<(bool, String)>,
}

impl Console {
    pub(super) fn new(enabled: bool) -> Self {
        let plain = StyleChoice::auto(false, true);
        Self {
            state: Mutex::new(ConsoleState {
                enabled,
                verbose: true,
                ..ConsoleState::default()
            }),
            stdout_style: if enabled { stdout_style() } else { plain },
            stderr_style: if enabled { stderr_style() } else { plain },
        }
    }

    pub(super) fn lock(&self) -> MutexGuard<'_, ConsoleState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn emit(&self, kind: MsgKind, message: &str) {
        let error = matches!(kind, MsgKind::Warn | MsgKind::Error);
        let style = if error {
            self.stderr_style
        } else {
            self.stdout_style
        };
        let mut state = self.lock();
        if let Some(line) = ui_line_with_style(kind, message, style, state.verbose) {
            state.line(error, &line);
        }
    }

    pub(super) fn task_result(&self, message: &str) {
        self.lock().line(false, &self.stdout_style.clean(message));
    }

    #[cfg(test)]
    pub(super) fn captured_lines(&self) -> Vec<String> {
        self.lock()
            .captured
            .iter()
            .flat_map(|(_, text)| text.lines().map(str::to_string))
            .collect()
    }
}

impl ConsoleState {
    #[allow(
        clippy::needless_pass_by_ref_mut,
        reason = "writes require exclusive console access and mutate test capture"
    )]
    fn write(&mut self, error: bool, text: &str) {
        #[cfg(test)]
        self.captured.push((error, text.to_string()));
        if self.enabled {
            if error {
                drop(std::io::stderr().lock().write_all(text.as_bytes()));
            } else {
                let mut out = std::io::stdout().lock();
                drop(out.write_all(text.as_bytes()));
                drop(out.flush());
            }
        }
    }

    fn line(&mut self, error: bool, line: &str) {
        let clear = progress_clear_sequence(std::mem::take(&mut self.progress_rows));
        self.ends_with_blank_line = strip_ansi(line).trim().is_empty();
        self.write(error, &format!("{clear}{line}\n"));
    }

    pub(super) fn clear_progress(&mut self) {
        let rows = std::mem::take(&mut self.progress_rows);
        if rows > 0 {
            self.write(false, &progress_clear_sequence(rows));
        }
    }

    /// Replace the status row, preserving any transient spacer above it.
    pub(super) fn replace_status(&mut self, line: &str) {
        self.progress_rows = self.progress_rows.max(1);
        self.write(false, &format!("\r\x1b[K{line}"));
    }

    /// Redraw active work with exactly one separator after durable output.
    pub(super) fn active_status(&mut self, line: &str) {
        if self.progress_rows > 0 {
            self.replace_status(line);
        } else {
            self.separate_from_startup();
            let prefix = if self.ends_with_blank_line { "" } else { "\n" };
            self.progress_rows = if prefix.is_empty() { 1 } else { 2 };
            self.write(false, &format!("{prefix}{line}"));
        }
    }

    pub(super) fn separate_from_startup(&mut self) {
        if !self.startup_separator_emitted {
            self.startup_separator_emitted = true;
            if !self.ends_with_blank_line {
                self.line(false, "");
            }
        }
    }

    pub(super) const fn should_separate_task_blocks(&self, expanded: bool, spaced: bool) -> bool {
        self.task_output_emitted && (spaced || self.last_task_block_had_details || expanded)
    }

    pub(super) fn begin_task_block(&mut self, expanded: bool, spaced: bool) {
        self.separate_from_startup();
        if self.should_separate_task_blocks(expanded, spaced) {
            self.line(false, "");
        }
    }

    pub(super) const fn end_task_block(&mut self, expanded: bool) {
        self.last_task_block_had_details = expanded;
        self.task_output_emitted = true;
    }
}

pub(in crate::infra::logging) fn progress_clear_sequence(rows: u16) -> String {
    if rows == 0 {
        return String::new();
    }

    let mut clear = String::from("\r\x1b[K");
    for _ in 1..usize::from(rows) {
        clear.push_str("\x1b[1A\r\x1b[K");
    }
    clear
}

pub(in crate::infra::logging) fn ui_line_with_style(
    kind: MsgKind,
    msg: &str,
    style: StyleChoice,
    verbose: bool,
) -> Option<String> {
    let msg = style.clean(msg);
    match kind {
        MsgKind::Stage | MsgKind::TaskStage => verbose.then_some(msg),
        MsgKind::Info | MsgKind::Debug => verbose.then(|| format!("  {msg}")),
        MsgKind::Context => verbose.then(|| style.paint(TextStyle::Dim, &msg)),
        MsgKind::Trace | MsgKind::Summary => None,
        MsgKind::Warn => Some(format!("{}  {msg}", style.paint(TextStyle::Yellow, "WARN"))),
        MsgKind::Error => Some(format!("{} {msg}", style.paint(TextStyle::Red, "ERROR"))),
        MsgKind::DryRun => Some(format!("  {msg}")),
        MsgKind::Always => Some(msg),
        MsgKind::Startup => Some(startup_line_with_style(&msg, style)),
    }
}

/// Give structured startup notices a visible label while keeping their
/// metadata subdued. Unstructured hints retain the original dim treatment.
fn startup_line_with_style(msg: &str, style: StyleChoice) -> String {
    let Some((label, metadata)) = msg.split_once(" · ") else {
        return style.paint(TextStyle::Dim, msg);
    };
    format!(
        "{}{}",
        style.paint(TextStyle::Bold, label),
        style.paint(TextStyle::Dim, &format!(" · {metadata}"))
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn progress_clear_sequences_cover_each_row_once() {
        for (rows, expected) in [
            (0, ""),
            (1, "\r\x1b[K"),
            (2, "\r\x1b[K\x1b[1A\r\x1b[K"),
            (3, "\r\x1b[K\x1b[1A\r\x1b[K\x1b[1A\r\x1b[K"),
        ] {
            assert_eq!(
                super::progress_clear_sequence(rows),
                expected,
                "{rows} rows"
            );
        }
    }
}
