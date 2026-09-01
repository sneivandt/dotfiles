//! Console event formatting and transient progress state.

use std::io::IsTerminal as _;
use std::io::Write as _;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};

use super::event::MessageExtractor;
use crate::infra::logging::style::{StyleChoice, TextStyle, stderr_style, stdout_style};
use crate::infra::logging::types::MsgKind;

/// Whether verbose console output is enabled.
///
/// Set once by `init_subscriber` and checked by [`DotfilesFormatter`] to
/// decide whether stage headers and plain info messages appear on the console.
static VERBOSE: AtomicBool = AtomicBool::new(true);
static TRANSIENT_PROGRESS_ROWS: AtomicU16 = AtomicU16::new(0);

/// Update the global verbose flag.
///
/// Called by `Logger::set_verbose` so that the formatter and file layer stay in
/// sync with the logger.
pub(in crate::infra::logging) fn set_verbose(verbose: bool) {
    VERBOSE.store(verbose, Ordering::Relaxed);
}

pub(in crate::infra::logging) fn set_transient_progress(rows: u16) {
    TRANSIENT_PROGRESS_ROWS.store(rows, Ordering::Relaxed);
}

pub(in crate::infra::logging) fn transient_progress_rows() -> u16 {
    TRANSIENT_PROGRESS_ROWS.load(Ordering::Relaxed)
}

pub(in crate::infra::logging) fn take_transient_progress_rows() -> u16 {
    TRANSIENT_PROGRESS_ROWS.swap(0, Ordering::Relaxed)
}

/// A [`tracing_subscriber::fmt::FormatEvent`] that emits dotfiles-style
/// console output.
pub(super) struct DotfilesFormatter;

fn progress_clear_sequence(rows: u16) -> String {
    if rows == 0 {
        return String::new();
    }

    let mut clear = String::from("\r\x1b[K");
    for _ in 1..usize::from(rows) {
        clear.push_str("\x1b[1A\r\x1b[K");
    }
    clear
}

fn clear_transient_console_prefix() -> String {
    if !std::io::stdout().is_terminal() {
        return String::new();
    }

    progress_clear_sequence(take_transient_progress_rows())
}

pub(super) fn ui_line_with_style(
    kind: MsgKind,
    msg: &str,
    style: StyleChoice,
    verbose: bool,
) -> Option<String> {
    let msg = style.clean(msg);
    match kind {
        MsgKind::Stage | MsgKind::TaskStage => verbose.then_some(msg),
        MsgKind::Info | MsgKind::Debug => verbose.then(|| format!("  {msg}")),
        MsgKind::Trace => None,
        MsgKind::Warn => Some(format!("{}  {msg}", style.paint(TextStyle::Yellow, "WARN"))),
        MsgKind::Error => Some(format!("{} {msg}", style.paint(TextStyle::Red, "ERROR"))),
        MsgKind::DryRun => Some(format!("  {msg}")),
        MsgKind::Always => Some(msg),
        MsgKind::Startup => Some(style.paint(TextStyle::Dim, &msg)),
    }
}

/// Render a user-facing logger message without routing it through `tracing`.
pub(in crate::infra::logging) fn emit_console(kind: MsgKind, msg: &str, verbose: bool) {
    let is_error = matches!(kind, MsgKind::Warn | MsgKind::Error);
    let style = if is_error {
        stderr_style()
    } else {
        stdout_style()
    };
    let Some(line) = ui_line_with_style(kind, msg, style, verbose) else {
        return;
    };
    let prefix = clear_transient_console_prefix();
    if is_error {
        let mut stream = std::io::stderr().lock();
        drop(writeln!(stream, "{prefix}{line}"));
    } else {
        let mut stream = std::io::stdout().lock();
        drop(writeln!(stream, "{prefix}{line}"));
    }
}

/// Render a compact task result line.
pub(in crate::infra::logging) fn emit_task_result(msg: &str) {
    let line = stdout_style().clean(msg);
    let prefix = clear_transient_console_prefix();
    let mut stream = std::io::stdout().lock();
    drop(writeln!(stream, "{prefix}{line}"));
}

fn console_style(level: tracing::Level) -> StyleChoice {
    if matches!(level, tracing::Level::ERROR | tracing::Level::WARN) {
        stderr_style()
    } else {
        stdout_style()
    }
}

fn console_line(level: tracing::Level, target: &str, msg: &str) -> Option<String> {
    console_line_with_style(
        level,
        target,
        msg,
        console_style(level),
        VERBOSE.load(Ordering::Relaxed),
    )
}

pub(super) fn console_line_with_style(
    level: tracing::Level,
    _target: &str,
    msg: &str,
    style: StyleChoice,
    verbose: bool,
) -> Option<String> {
    let msg = style.clean(msg);
    match level {
        tracing::Level::ERROR => Some(format!("{} {msg}", style.paint(TextStyle::Red, "ERROR"))),
        tracing::Level::WARN => Some(format!("{}  {msg}", style.paint(TextStyle::Yellow, "WARN"))),
        tracing::Level::INFO => verbose.then(|| format!("  {msg}")),
        tracing::Level::DEBUG | tracing::Level::TRACE => None,
    }
}

impl<S, N> tracing_subscriber::fmt::FormatEvent<S, N> for DotfilesFormatter
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    N: for<'a> tracing_subscriber::fmt::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: tracing_subscriber::fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let metadata = event.metadata();
        let level = *metadata.level();
        let target = metadata.target();

        let mut extractor = MessageExtractor::default();
        event.record(&mut extractor);
        let msg = &extractor.message;

        let Some(line) = console_line(level, target, msg) else {
            return Ok(());
        };
        write!(writer, "{}", clear_transient_console_prefix())?;
        writeln!(writer, "{line}")
    }
}
