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
    pub(super) stderr_style: StyleChoice,
}

/// All terminal state changes and writes happen under the same lock.
#[derive(Debug, Default)]
pub(super) struct ConsoleState {
    enabled: bool,
    pub(super) mode: super::OutputMode,
    pub(super) run_id: Option<String>,
    output_error: Option<std::io::Error>,
    stdout_closed: bool,
    stderr_closed: bool,
    pub(super) verbose: bool,
    pub(super) progress_rows: u16,
    /// A live task footer is separate from a pre-graph custom transient.
    pub(super) active_progress: bool,
    pub(super) custom_progress: Option<String>,
    pub(super) task_output_emitted: bool,
    pub(super) last_task_block_had_details: bool,
    pub(super) ends_with_blank_line: bool,
    pub(super) startup_separator_emitted: bool,
    pub(super) startup_header_emitted: bool,
    pub(super) last_was_transition: bool,
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

    pub(super) fn emit(&self, kind: MsgKind, message: &str, symbols: bool) {
        let error = matches!(
            kind.log_event(),
            super::types::LogEvent::Warn | super::types::LogEvent::Error
        );
        let style = if error {
            self.stderr_style
        } else {
            self.stdout_style
        };
        if error {
            self.record(
                &super::log_thread_name(),
                super::records::Record::Message {
                    event: kind.log_event().name().into(),
                    text: message.into(),
                },
            );
        }
        let mut state = self.lock();
        if let Some(line) = ui_line_with_style(kind, message, style, state.verbose, symbols) {
            if kind == MsgKind::Transition && !state.last_was_transition {
                if state.startup_header_emitted {
                    state.separate_from_startup();
                }
                if state.startup_separator_emitted {
                    state.separate_block();
                }
            }
            state.line(error, &line);
            state.last_was_transition = kind == MsgKind::Transition;
            state.startup_header_emitted |= kind == MsgKind::Startup;
        }
    }

    /// Emit a versioned structured fact without human text or ANSI escapes.
    pub(super) fn record(&self, context: &str, record: super::records::Record) {
        let mut state = self.lock();
        if state.mode != super::OutputMode::Jsonl {
            return;
        }
        match serde_json::to_value(super::records::StoredRecord::new(context, record)) {
            Ok(mut value) => {
                if let Some(fields) = value.as_object_mut() {
                    fields.insert("run_id".into(), serde_json::json!(state.run_id));
                }
                state.write_raw(false, &format!("{value}\n"));
            }
            Err(error) => state.remember_error(std::io::Error::other(error)),
        }
    }

    /// Report a broken output sink through stderr even in structured mode.
    pub(super) fn report_output_error(&self, message: &str) {
        self.lock().write_raw(true, &format!("ERROR {message}\n"));
    }

    pub(super) fn output_error(&self) -> Option<String> {
        self.lock().output_error.as_ref().map(ToString::to_string)
    }

    pub(super) fn task_result(&self, message: &str) {
        self.lock().line(false, &self.stdout_style.clean(message));
    }

    /// Render a buffered detail without persisting its presentation.
    pub(super) fn detail(&self, kind: MsgKind, message: &str, line: &str, error_stream: bool) {
        let diagnostic = matches!(
            kind.log_event(),
            super::types::LogEvent::Warn | super::types::LogEvent::Error
        );
        if diagnostic {
            self.record(
                &super::log_thread_name(),
                super::records::Record::Message {
                    event: kind.log_event().name().into(),
                    text: message.into(),
                },
            );
        }
        self.lock().line(error_stream, line);
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
        if self.mode == super::OutputMode::Jsonl
            || (self.mode == super::OutputMode::Quiet && !error)
        {
            return;
        }
        self.write_raw(error, text);
    }

    fn write_raw(&mut self, error: bool, text: &str) {
        #[cfg(test)]
        self.captured.push((error, text.to_string()));
        self.write_using(error, || {
            if error {
                let mut out = std::io::stderr().lock();
                out.write_all(text.as_bytes()).and_then(|()| out.flush())
            } else {
                let mut out = std::io::stdout().lock();
                out.write_all(text.as_bytes()).and_then(|()| out.flush())
            }
        });
    }

    fn write_using(&mut self, error: bool, write: impl FnOnce() -> std::io::Result<()>) {
        if !self.enabled
            || (if error {
                self.stderr_closed
            } else {
                self.stdout_closed
            })
        {
            return;
        }
        let result = write();
        if let Err(failure) = result {
            if error {
                self.stderr_closed = true;
            } else {
                self.stdout_closed = true;
            }
            self.remember_error(failure);
        }
    }

    fn remember_error(&mut self, error: std::io::Error) {
        // A consumer may intentionally stop reading. Keep convergence running;
        // other write failures must affect the command's final exit status.
        if error.kind() != std::io::ErrorKind::BrokenPipe && self.output_error.is_none() {
            self.output_error = Some(error);
        }
    }

    fn line(&mut self, error: bool, line: &str) {
        self.last_was_transition = false;
        self.active_progress = false;
        self.custom_progress = None;
        let clear = progress_clear_sequence(std::mem::take(&mut self.progress_rows));
        self.ends_with_blank_line = strip_ansi(line).trim().is_empty();
        self.write(error, &format!("{clear}{line}\n"));
    }

    pub(super) fn clear_progress(&mut self) {
        self.active_progress = false;
        self.custom_progress = None;
        let rows = std::mem::take(&mut self.progress_rows);
        if rows > 0 {
            self.write(false, &progress_clear_sequence(rows));
        }
    }

    /// Replace the status row, preserving any transient spacer above it.
    pub(super) fn replace_status(&mut self, line: &str) {
        self.active_progress = false;
        if self.mode != super::OutputMode::Human {
            return;
        }
        self.progress_rows = self.progress_rows.max(1);
        self.write(false, &format!("\r\x1b[K{line}"));
    }

    /// Leave a spacer after ordinary output, but keep lifecycle notices together.
    pub(super) fn active_status(&mut self, line: &str) {
        if self.mode != super::OutputMode::Human {
            return;
        }
        self.custom_progress = None;
        if self.progress_rows > 0 || self.last_was_transition {
            self.replace_status(line);
        } else {
            self.separate_from_startup();
            let prefix = if self.ends_with_blank_line { "" } else { "\n" };
            self.progress_rows = if prefix.is_empty() { 1 } else { 2 };
            self.write(false, &format!("{prefix}{line}"));
        }
        self.active_progress = true;
    }

    pub(super) fn separate_from_startup(&mut self) {
        if !self.startup_separator_emitted {
            self.startup_separator_emitted = true;
            if !self.ends_with_blank_line {
                self.line(false, "");
            }
        }
    }

    pub(super) fn separate_block(&mut self) {
        if !self.ends_with_blank_line {
            self.line(false, "");
        }
    }

    pub(super) const fn should_separate_task_blocks(&self, expanded: bool) -> bool {
        self.task_output_emitted && (self.last_task_block_had_details || expanded)
    }

    pub(super) fn begin_task_block(&mut self, expanded: bool) {
        if self.last_was_transition {
            self.separate_block();
        }
        self.separate_from_startup();
        if self.should_separate_task_blocks(expanded) {
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
    symbols: bool,
) -> Option<String> {
    let msg = style.clean(msg);
    match kind {
        MsgKind::Stage => verbose.then_some(msg),
        MsgKind::Info => verbose.then(|| format!("  {msg}")),
        MsgKind::Debug => verbose.then(|| style.paint(TextStyle::Dim, &format!("  {msg}"))),
        MsgKind::Context => verbose.then(|| style.paint(TextStyle::Dim, &msg)),
        MsgKind::Trace | MsgKind::Summary => None,
        MsgKind::Warn => Some(format!(
            "{}  {msg}",
            style.paint_bold(TextStyle::Yellow, "WARN")
        )),
        MsgKind::Error => Some(format!(
            "{} {msg}",
            style.paint_bold(TextStyle::Red, "ERROR")
        )),
        MsgKind::DryRun => Some(format!("  {msg}")),
        MsgKind::Always | MsgKind::WarnContinuation => Some(msg),
        MsgKind::Startup => Some(startup_line_with_style(&msg, style)),
        MsgKind::Transition => {
            let (label, metadata) = msg.split_once(" · ").unwrap_or((&msg, ""));
            let mut line = style.bold(label);
            if !metadata.is_empty() {
                let current = metadata.split_once(" → ").map_or("", |(_, new)| new);
                let muted = &metadata[..metadata.len() - current.len()];
                line.push_str(&style.paint(TextStyle::Dim, &format!(" · {muted}")));
                line.push_str(current);
            }
            Some(line)
        }
        MsgKind::RecoveryHint => {
            let command = msg
                .strip_prefix("Run '")
                .and_then(|text| text.strip_suffix("' for details."));
            let Some(command) = command else {
                return Some(format!("  {msg}"));
            };
            Some(format!(
                "  {}{}{}",
                style.paint(TextStyle::Dim, "Run "),
                style.paint(TextStyle::Cyan, &format!("'{command}'")),
                style.paint(TextStyle::Dim, " for details."),
            ))
        }
        MsgKind::SudoRequired => {
            let notice = startup_line_with_style(&msg, style);
            Some(if symbols {
                format!("{} {notice}", style.paint(TextStyle::Yellow, "⊙"))
            } else {
                notice
            })
        }
    }
}

/// Give the run header a visible command while subduing its metadata.
fn startup_line_with_style(msg: &str, style: StyleChoice) -> String {
    let Some((label, metadata)) = msg.split_once(" · ") else {
        return style.clean(msg);
    };
    let metadata = metadata.strip_prefix("dry run · ").map_or_else(
        || style.paint(TextStyle::Dim, &format!(" · {metadata}")),
        |rest| {
            format!(
                "{}{}{}",
                style.paint(TextStyle::Dim, " · "),
                style.paint(TextStyle::Magenta, "dry run"),
                style.paint(TextStyle::Dim, &format!(" · {rest}")),
            )
        },
    );
    format!("{}{metadata}", style.paint(TextStyle::Bold, label))
}

#[cfg(test)]
mod tests {
    #[test]
    fn disabled_or_closed_sinks_are_never_written_again() {
        for error_stream in [false, true] {
            let writes = std::cell::Cell::new(0_u32);
            let mut state = super::ConsoleState::default();
            state.write_using(error_stream, || {
                writes.set(writes.get().saturating_add(1));
                Ok(())
            });
            assert_eq!(writes.get(), 0, "disabled sink");
            state.enabled = true;
            state.write_using(error_stream, || {
                writes.set(writes.get().saturating_add(1));
                Err(std::io::ErrorKind::BrokenPipe.into())
            });
            state.write_using(error_stream, || {
                writes.set(writes.get().saturating_add(1));
                Ok(())
            });
            assert_eq!(writes.get(), 1, "closed sink must not retry");
            assert!(state.output_error.is_none(), "broken pipe is benign");
        }
    }

    #[test]
    fn write_failures_close_only_the_affected_stream_and_retain_the_first_real_error() {
        use std::io::{Error, ErrorKind};

        for error_stream in [false, true] {
            for first_kind in [ErrorKind::BrokenPipe, ErrorKind::WriteZero] {
                let mut state = super::ConsoleState {
                    enabled: true,
                    ..super::ConsoleState::default()
                };
                state.write_using(error_stream, || {
                    Err(Error::new(first_kind, "first failure"))
                });
                state.write_using(error_stream, || {
                    panic!("a failed stream must not be retried")
                });
                assert_eq!(state.stderr_closed, error_stream);
                assert_eq!(state.stdout_closed, !error_stream);
                assert_eq!(
                    state.output_error.as_ref().map(Error::kind),
                    (first_kind != ErrorKind::BrokenPipe).then_some(first_kind)
                );

                let writes = std::cell::Cell::new(0);
                state.write_using(!error_stream, || {
                    writes.set(writes.get() + 1);
                    Ok(())
                });
                assert_eq!(writes.get(), 1, "the other stream must remain usable");
                state.write_using(!error_stream, || {
                    Err(Error::new(ErrorKind::PermissionDenied, "second failure"))
                });
                state.write_using(!error_stream, || {
                    panic!("both failed streams must stay closed")
                });
                assert!(state.stdout_closed && state.stderr_closed);
                assert_eq!(
                    state
                        .output_error
                        .as_ref()
                        .map(ToString::to_string)
                        .as_deref(),
                    Some(if first_kind == ErrorKind::BrokenPipe {
                        "second failure"
                    } else {
                        "first failure"
                    }),
                    "a broken pipe must not mask a later actionable failure"
                );
            }
        }
    }

    #[test]
    fn finishing_a_run_preserves_output_failures_in_exit_status_and_persistent_records() {
        use super::super::records::{Record, RunOutcome, StoredRecord};
        use super::super::{Logger, Output, OutputMode};
        use std::io::{Error, ErrorKind};

        for (error_stream, kind, expected_code, expected_outcome) in [
            (false, ErrorKind::BrokenPipe, 0, RunOutcome::Succeeded),
            (true, ErrorKind::BrokenPipe, 0, RunOutcome::Succeeded),
            (false, ErrorKind::WriteZero, 1, RunOutcome::Failed),
            (true, ErrorKind::PermissionDenied, 1, RunOutcome::Failed),
        ] {
            for mode in [OutputMode::Human, OutputMode::Quiet, OutputMode::Jsonl] {
                let root = tempfile::tempdir_in(".").unwrap();
                let mut log = Logger::new_in("update", root.path());
                log.set_output_mode(mode);
                log.start_run(None);
                {
                    let mut state = log.console.lock();
                    state.enabled = true;
                    state.write_using(error_stream, || {
                        Err(Error::new(kind, "fixture output failure"))
                    });
                    state.enabled = false;
                }

                assert_eq!(log.finish_run(0), expected_code, "{mode:?}: {kind:?}");
                assert_eq!(log.finish_run(0), expected_code, "repeated completion");
                let content = std::fs::read_to_string(log.log_path().unwrap()).unwrap();
                let finishes: Vec<_> = content
                    .lines()
                    .filter_map(StoredRecord::from_line)
                    .filter_map(|stored| {
                        if let Record::RunFinish {
                            outcome, exit_code, ..
                        } = stored.record
                        {
                            Some((outcome, exit_code))
                        } else {
                            None
                        }
                    })
                    .collect();
                assert_eq!(
                    finishes,
                    [(expected_outcome, expected_code)],
                    "{mode:?}: retained completion must describe the effective outcome exactly once"
                );
                assert_eq!(
                    content.contains("writing command output: fixture output failure"),
                    kind != ErrorKind::BrokenPipe,
                    "{mode:?}: only actionable output failures belong in diagnostics"
                );
            }
        }
    }

    #[test]
    fn progress_redraws_replace_the_active_row_only_in_human_mode() {
        for mode in [
            super::super::OutputMode::Human,
            super::super::OutputMode::Quiet,
            super::super::OutputMode::Jsonl,
        ] {
            let mut state = super::ConsoleState {
                mode,
                startup_separator_emitted: true,
                ..super::ConsoleState::default()
            };
            state.active_status("Running old task");
            state.active_status("Running new task");
            if mode == super::super::OutputMode::Human {
                assert_eq!(
                    state.captured.last(),
                    Some(&(false, "\r\x1b[KRunning new task".into()))
                );
                assert_eq!(state.progress_rows, 2);
            } else {
                assert!(state.captured.is_empty());
                assert_eq!(state.progress_rows, 0);
            }
        }
    }

    #[test]
    fn sync_progress_and_notice_share_the_row_below_the_upgrade_notice() {
        use super::{Console, MsgKind};

        let console = Console::new(false);
        console.emit(MsgKind::Startup, "Update", false);
        console.emit(MsgKind::Transition, "CLI upgraded", false);
        console.lock().active_status("Syncing repository");
        console.emit(MsgKind::Transition, "Repository synced", false);

        let output: String = console
            .lock()
            .captured
            .iter()
            .map(|(_, text)| text.as_str())
            .collect();
        assert_eq!(
            output,
            "Update\n\nCLI upgraded\n\r\x1b[KSyncing repository\r\x1b[KRepository synced\n"
        );
    }

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
