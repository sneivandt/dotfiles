//! Tracing subscriber setup: console formatter, file layer, and initialisation.
use std::fs;
use std::io::IsTerminal as _;
use std::io::Write as _;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};

use super::style::{StyleChoice, TextStyle, stderr_style, stdout_style};
use super::utils::{format_utc_datetime, format_utc_time, log_file_path, strip_ansi};

/// Whether verbose console output is enabled.
///
/// Set once by [`init_subscriber`] and checked by [`DotfilesFormatter`] to
/// decide whether stage headers and plain info messages appear on the console.
static VERBOSE: AtomicBool = AtomicBool::new(true);
static TRANSIENT_PROGRESS_ROWS: AtomicU16 = AtomicU16::new(0);

/// Update the global verbose flag.
///
/// Called by [`Logger::set_verbose`](super::logger::Logger::set_verbose) so
/// that the formatter and file layer stay in sync with the logger.
pub(super) fn set_verbose(verbose: bool) {
    VERBOSE.store(verbose, Ordering::Relaxed);
}

pub(in crate::runtime::logging) fn set_transient_progress(rows: u16) {
    TRANSIENT_PROGRESS_ROWS.store(rows, Ordering::Relaxed);
}

pub(in crate::runtime::logging) fn transient_progress_rows() -> u16 {
    TRANSIENT_PROGRESS_ROWS.load(Ordering::Relaxed)
}

pub(in crate::runtime::logging) fn take_transient_progress_rows() -> u16 {
    TRANSIENT_PROGRESS_ROWS.swap(0, Ordering::Relaxed)
}

/// Extracts the `message` field from a [`tracing::Event`].
#[derive(Default)]
struct MessageExtractor {
    message: String,
}

impl tracing::field::Visit for MessageExtractor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }
}

/// Task context stored on tracing spans created by `tasks::execute`.
#[derive(Debug, Default)]
struct TaskSpanContext {
    task_name: Option<String>,
}

/// Extracts the task name from a tracing span's `name` field.
#[derive(Default)]
struct SpanContextExtractor {
    task_name: Option<String>,
}

impl tracing::field::Visit for SpanContextExtractor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "name" {
            let rendered = format!("{value:?}");
            let rendered_value = rendered
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(&rendered);
            self.task_name = Some(rendered_value.to_string());
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "name" {
            self.task_name = Some(value.to_string());
        }
    }
}

/// A [`tracing_subscriber::Layer`] that appends all events to the persistent
/// log file with timestamps and ANSI codes stripped.
///
/// Created by [`init_subscriber`] so that file output goes through the same
/// tracing pipeline as console output.  Always captures events at `DEBUG`
/// level and above regardless of the console verbosity setting.
#[derive(Debug)]
pub(super) struct FileLayer {
    file: Mutex<fs::File>,
}

impl FileLayer {
    /// Open (or create) the log file for `command`, write a run header, and
    /// return a new `FileLayer` ready to receive events.
    ///
    /// Returns `None` if the cache directory cannot be created or the file
    /// cannot be opened.
    pub(super) fn new(command: &str) -> Option<Self> {
        let path = log_file_path(command)?;
        Self::create_at(&path)
    }

    /// Open (or create) the log file for `command` under `cache_dir`, write
    /// a run header, and return a new `FileLayer`.
    ///
    /// Like [`new`](Self::new) but uses an explicit cache base directory
    /// instead of reading `XDG_CACHE_HOME` from the environment.
    #[cfg(test)]
    pub(super) fn new_in(command: &str, cache_dir: &std::path::Path) -> Option<Self> {
        let path = super::utils::log_file_path_in(command, cache_dir)?;
        Self::create_at(&path)
    }

    /// Shared implementation: write a header and open the file for appending.
    #[allow(clippy::print_stderr, reason = "intentional user-facing output")]
    fn create_at(path: &std::path::Path) -> Option<Self> {
        use std::io::Write as _;

        let version =
            option_env!("DOTFILES_VERSION").unwrap_or(concat!("dev-", env!("CARGO_PKG_VERSION")));
        let header = format!(
            "==========================================\n\
             Dotfiles {version} {}\n\
             ==========================================\n",
            format_utc_datetime(),
        );
        let mut file = match fs::File::create(path) {
            Ok(f) => f,
            Err(err) => {
                eprintln!("Warning: failed to initialize log file: {err}");
                return None;
            }
        };
        if let Err(err) = file.write_all(header.as_bytes()) {
            eprintln!("Warning: failed to initialize log file: {err}");
            return None;
        }
        Some(Self {
            file: Mutex::new(file),
        })
    }
}

impl<S> tracing_subscriber::Layer<S> for FileLayer
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut extractor = SpanContextExtractor::default();
        attrs.record(&mut extractor);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(TaskSpanContext {
                task_name: extractor.task_name,
            });
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        let metadata = event.metadata();
        let level = *metadata.level();
        let target = metadata.target();

        if level == tracing::Level::INFO && target == "dotfiles::task_result" {
            return;
        }

        let mut extractor = MessageExtractor::default();
        event.record(&mut extractor);
        let raw = strip_ansi(&extractor.message);
        let msg = raw.trim_start();

        if msg.is_empty() {
            return;
        }

        let task_name = event_task_name(event, &ctx);
        let ts = format_utc_time();
        let context = task_name.map_or_else(String::new, |name| format!(" [{name}]"));
        let level_label = log_level_label(level, target);
        let prefix = format!("[{ts}]{context} [{level_label}]");

        let line = match (level, target) {
            (tracing::Level::INFO, "dotfiles::stage") => format!("{prefix} ==> {msg}"),
            (tracing::Level::INFO, "dotfiles::file_only_stage") => {
                format!("{prefix} ==> {msg}")
            }
            _ => format!("{prefix} {msg}"),
        };

        if let Ok(mut f) = self.file.lock() {
            drop(writeln!(f, "{line}"));
        }
    }
}

fn log_level_label(level: tracing::Level, target: &str) -> &'static str {
    match (level, target) {
        (tracing::Level::INFO, "dotfiles::file_only_error") | (tracing::Level::ERROR, _) => "error",
        (tracing::Level::INFO, "dotfiles::file_only_warn") | (tracing::Level::WARN, _) => "warn",
        (tracing::Level::INFO, "dotfiles::file_only_debug") | (tracing::Level::DEBUG, _) => "debug",
        (tracing::Level::INFO, "dotfiles::stage" | "dotfiles::file_only_stage") => "stage",
        (tracing::Level::INFO, _) => "info",
        (tracing::Level::TRACE, _) => "trace",
    }
}

fn event_task_name<S>(
    event: &tracing::Event<'_>,
    ctx: &tracing_subscriber::layer::Context<'_, S>,
) -> Option<String>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    let scope = ctx.event_scope(event)?;
    let mut task_name = None;
    for span in scope.from_root() {
        if span.metadata().name() == "task"
            && let Some(context) = span.extensions().get::<TaskSpanContext>()
        {
            task_name.clone_from(&context.task_name);
        }
    }
    task_name
}

/// A [`tracing_subscriber::fmt::FormatEvent`] that emits dotfiles-style
/// console output.
struct DotfilesFormatter;

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

fn console_style(level: tracing::Level) -> StyleChoice {
    if matches!(level, tracing::Level::ERROR | tracing::Level::WARN) {
        stderr_style()
    } else {
        stdout_style()
    }
}

fn console_line(level: tracing::Level, target: &str, msg: &str) -> Option<String> {
    console_line_with_style(level, target, msg, console_style(level))
}

fn console_line_with_style(
    level: tracing::Level,
    target: &str,
    msg: &str,
    style: StyleChoice,
) -> Option<String> {
    let msg = style.clean(msg);
    match level {
        _ if target.starts_with("dotfiles::file_only") => None,
        tracing::Level::ERROR => Some(format!("{} {msg}", style.paint(TextStyle::Red, "ERROR"))),
        tracing::Level::WARN => Some(format!("{}  {msg}", style.paint(TextStyle::Yellow, "WARN"))),
        tracing::Level::INFO if target == "dotfiles::always" => Some(msg),
        tracing::Level::INFO if target == "dotfiles::task_result" => Some(msg),
        tracing::Level::INFO if target == "dotfiles::stage" => VERBOSE
            .load(Ordering::Relaxed)
            .then(|| style.paint(TextStyle::Bold, &msg)),
        tracing::Level::INFO if target == "dotfiles::dry_run" => Some(format!("  {msg}")),
        tracing::Level::INFO => VERBOSE.load(Ordering::Relaxed).then(|| format!("  {msg}")),
        _ => Some(format!("  {}", style.paint(TextStyle::Dim, &msg))),
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

/// Initialise the global [`tracing`] subscriber.
///
/// Sets up a console subscriber that formats events to match the dotfiles
/// output style and a file subscriber that writes all events (including
/// `debug`) to `$XDG_CACHE_HOME/dotfiles/<command>.log`.
/// Must be called once at program startup, before any logging.
pub fn init_subscriber(verbose: bool, command: &str) {
    use tracing_subscriber::fmt::writer::MakeWriterExt as _;
    use tracing_subscriber::{
        Layer as _, filter::LevelFilter, fmt, layer::SubscriberExt as _,
        util::SubscriberInitExt as _,
    };

    VERBOSE.store(verbose, Ordering::Relaxed);

    let console_level = if verbose {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    };

    let make_writer = std::io::stderr
        .with_max_level(tracing::Level::WARN)
        .and(std::io::stdout.with_min_level(tracing::Level::INFO));

    let console_layer = fmt::layer()
        .event_format(DotfilesFormatter)
        .with_writer(make_writer)
        .with_filter(console_level);

    let file_layer = FileLayer::new(command).map(|l| l.with_filter(LevelFilter::DEBUG));

    tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .init();
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt as _;

    /// Create a [`FileLayer`] in a temp directory and return the log file path,
    /// temp dir (must outlive the layer), and a tracing dispatcher guard.
    fn isolated_file_layer() -> (
        std::path::PathBuf,
        tempfile::TempDir,
        super::super::TestDispatchGuard,
    ) {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let layer =
            FileLayer::new_in("test", tmp.path()).expect("FileLayer::new_in should succeed");
        let path = super::super::utils::log_file_path_in("test", tmp.path())
            .expect("log path should resolve");
        let subscriber = tracing_subscriber::registry().with(layer);
        let dispatch = tracing::Dispatch::new(subscriber);
        let guard = super::super::test_dispatch_guard(&dispatch);
        (path, tmp, guard)
    }

    // -----------------------------------------------------------------------
    // FileLayer::new — header
    // -----------------------------------------------------------------------

    #[test]
    fn file_layer_new_writes_header() {
        let (path, _tmp, _guard) = isolated_file_layer();
        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("=========================================="),
            "header should contain separator line"
        );
        assert!(
            content.contains("Dotfiles"),
            "header should contain 'Dotfiles'"
        );
    }

    // -----------------------------------------------------------------------
    // FileLayer::on_event — formatting
    // -----------------------------------------------------------------------

    #[test]
    fn file_layer_formats_stage_with_arrow() {
        let (path, _tmp, _guard) = isolated_file_layer();
        tracing::info!(target: "dotfiles::stage", "my stage");
        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("==> my stage"),
            "stage should be prefixed with ==>: {content}"
        );
    }

    #[test]
    fn file_layer_formats_dry_run_without_tag() {
        let (path, _tmp, _guard) = isolated_file_layer();
        tracing::info!(target: "dotfiles::dry_run", "would link");
        let content = fs::read_to_string(&path).unwrap();
        let line = content.lines().find(|l| l.contains("would link")).unwrap();
        assert!(
            !line.contains("[dry run]"),
            "dry_run should not have [dry run] tag in file: {line}"
        );
    }

    #[test]
    fn file_layer_formats_error_level_label() {
        let (path, _tmp, _guard) = isolated_file_layer();
        tracing::error!("something broke");
        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("[error] something broke"),
            "error should have text level label: {content}"
        );
    }

    #[test]
    fn file_layer_formats_warn_level_label() {
        let (path, _tmp, _guard) = isolated_file_layer();
        tracing::warn!("careful now");
        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("[warn] careful now"),
            "warn should have text level label: {content}"
        );
    }

    #[test]
    fn file_layer_formats_debug_with_level_label() {
        let (path, _tmp, _guard) = isolated_file_layer();
        tracing::debug!("extra detail");
        let content = fs::read_to_string(&path).unwrap();
        let line = content
            .lines()
            .find(|l| l.contains("extra detail"))
            .unwrap();
        assert!(
            line.contains("[debug]"),
            "debug should have text level label: {line}"
        );
    }

    #[test]
    fn file_layer_formats_info_with_level_label() {
        let (path, _tmp, _guard) = isolated_file_layer();
        tracing::info!("regular info");
        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("regular info"),
            "info message should appear: {content}"
        );
        let info_line = content
            .lines()
            .find(|l| l.contains("regular info"))
            .unwrap();
        assert!(
            info_line.contains("[info]") && !info_line.contains("==>"),
            "plain info should have text level label and no stage marker: {info_line}"
        );
    }

    #[test]
    fn file_layer_formats_task_context_before_level_label() {
        let (path, _tmp, _guard) = isolated_file_layer();
        let span = tracing::info_span!("task", name = "example-task");
        let _enter = span.enter();

        tracing::info!("task detail");

        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("[example-task] [info] task detail"),
            "task context should precede level label: {content}"
        );
    }

    #[test]
    fn file_layer_strips_ansi_codes() {
        let (path, _tmp, _guard) = isolated_file_layer();
        tracing::info!("\x1b[31mcolored\x1b[0m text");
        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("colored text"),
            "ANSI codes should be stripped: {content}"
        );
        assert!(
            !content.contains("\x1b["),
            "no ANSI escape should remain: {content}"
        );
    }

    #[test]
    fn file_layer_includes_timestamp() {
        let (path, _tmp, _guard) = isolated_file_layer();
        tracing::info!("timestamped");
        let content = fs::read_to_string(&path).unwrap();
        let line = content.lines().find(|l| l.contains("timestamped")).unwrap();
        // Timestamp format: [HH:MM:SS]
        assert!(
            line.starts_with('['),
            "event line should start with timestamp bracket: {line}"
        );
    }

    #[test]
    fn file_layer_strips_leading_whitespace() {
        let (path, _tmp, _guard) = isolated_file_layer();
        tracing::info!("  padded info");
        tracing::debug!("    deep indent");
        tracing::warn!("  padded warn");
        let content = fs::read_to_string(&path).unwrap();

        let info_line = content.lines().find(|l| l.contains("padded info")).unwrap();
        assert!(
            info_line.ends_with("] padded info"),
            "leading whitespace should be stripped from info: {info_line}"
        );

        let debug_line = content.lines().find(|l| l.contains("deep indent")).unwrap();
        assert!(
            debug_line.ends_with("] deep indent"),
            "leading whitespace should be stripped from debug: {debug_line}"
        );

        let warn_line = content.lines().find(|l| l.contains("padded warn")).unwrap();
        assert!(
            warn_line.ends_with("[warn] padded warn"),
            "leading whitespace should be stripped from warn: {warn_line}"
        );
    }

    #[test]
    fn file_layer_omits_empty_messages() {
        let (path, _tmp, _guard) = isolated_file_layer();
        let before = fs::read_to_string(&path).unwrap();

        tracing::info!("");
        tracing::warn!("   ");
        tracing::error!("\t");
        tracing::debug!("\x1b[31m\x1b[0m");
        tracing::info!(target: "dotfiles::stage", "");
        tracing::info!(target: "dotfiles::dry_run", "  ");
        tracing::info!(target: "dotfiles::file_only", "");

        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(
            after, before,
            "empty log messages should not write timestamp-only file lines"
        );
    }

    #[test]
    fn console_line_uses_ansi_when_style_enabled() {
        let line = console_line_with_style(
            tracing::Level::WARN,
            "dotfiles",
            "careful",
            StyleChoice::colored(),
        )
        .unwrap();

        assert_eq!(line, "\x1b[33mWARN\x1b[0m  careful");
    }

    #[test]
    fn console_line_strips_ansi_when_style_disabled() {
        let line = console_line_with_style(
            tracing::Level::INFO,
            "dotfiles::always",
            "\x1b[32m3 Changed\x1b[0m",
            StyleChoice::plain(),
        )
        .unwrap();

        assert_eq!(line, "3 Changed");
    }

    #[test]
    fn console_line_plain_stderr_warning_has_no_ansi() {
        let line = console_line_with_style(
            tracing::Level::WARN,
            "dotfiles",
            "\x1b[1mcareful\x1b[0m",
            StyleChoice::plain(),
        )
        .unwrap();

        assert_eq!(line, "WARN  careful");
        assert!(!line.contains("\x1b["));
    }
}
