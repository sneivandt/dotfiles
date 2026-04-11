//! Tracing subscriber setup: console formatter, file layer, and initialisation.
use std::fs;
use std::io::Write as _;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use super::utils::{format_utc_datetime, format_utc_time, log_file_path, strip_ansi};

/// Whether verbose console output is enabled.
///
/// Set once by [`init_subscriber`] and checked by [`DotfilesFormatter`] to
/// decide whether stage headers and plain info messages appear on the console.
static VERBOSE: AtomicBool = AtomicBool::new(true);

/// Update the global verbose flag.
///
/// Called by [`Logger::set_verbose`](super::logger::Logger::set_verbose) so
/// that the formatter and file layer stay in sync with the logger.
pub(super) fn set_verbose(verbose: bool) {
    VERBOSE.store(verbose, Ordering::Relaxed);
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
    #[allow(clippy::print_stderr)]
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

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for FileLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();
        let level = *metadata.level();
        let target = metadata.target();

        let mut extractor = MessageExtractor::default();
        event.record(&mut extractor);
        let msg = strip_ansi(&extractor.message);
        let ts = format_utc_time();

        let line = match (level, target) {
            (tracing::Level::INFO, "dotfiles::task_result") => return,
            (tracing::Level::INFO, "dotfiles::stage") => format!("[{ts}] ==> {msg}"),
            (tracing::Level::INFO, "dotfiles::phase") => {
                format!("[{ts}] :: {msg}")
            }
            (tracing::Level::ERROR, _) => format!("[{ts}]  [error] {msg}"),
            (tracing::Level::WARN, _) => format!("[{ts}]  [warn] {msg}"),
            _ => format!("[{ts}]  {msg}"),
        };

        if let Ok(mut f) = self.file.lock() {
            writeln!(f, "{line}").ok();
        }
    }
}

/// A [`tracing_subscriber::fmt::FormatEvent`] that emits dotfiles-style
/// console output.
struct DotfilesFormatter;

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

        match level {
            tracing::Level::ERROR => writeln!(writer, "\x1b[31mERROR\x1b[0m {msg}"),
            tracing::Level::WARN => writeln!(writer, "\x1b[33mWARN\x1b[0m  {msg}"),
            tracing::Level::INFO if target == "dotfiles::always" => {
                writeln!(writer, "{msg}")
            }
            tracing::Level::INFO if target == "dotfiles::task_result" => {
                writeln!(writer, "{msg}")
            }
            tracing::Level::INFO if target == "dotfiles::stage" => {
                if VERBOSE.load(Ordering::Relaxed) {
                    writeln!(writer, "\x1b[1m{msg}\x1b[0m")
                } else {
                    Ok(())
                }
            }
            tracing::Level::INFO if target == "dotfiles::phase" => {
                writeln!(writer, "\x1b[1;34m::\x1b[0m \x1b[1;34m{msg}\x1b[0m")
            }
            tracing::Level::INFO if target == "dotfiles::dry_run" => {
                writeln!(writer, "  {msg}")
            }
            tracing::Level::INFO => {
                if VERBOSE.load(Ordering::Relaxed) {
                    writeln!(writer, "  {msg}")
                } else {
                    Ok(())
                }
            }
            _ => writeln!(writer, "  \x1b[2m{msg}\x1b[0m"),
        }
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
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt as _;

    /// Create a [`FileLayer`] in a temp directory and return the log file path,
    /// temp dir (must outlive the layer), and a tracing dispatcher guard.
    fn isolated_file_layer() -> (
        std::path::PathBuf,
        tempfile::TempDir,
        tracing::dispatcher::DefaultGuard,
    ) {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let layer =
            FileLayer::new_in("test", tmp.path()).expect("FileLayer::new_in should succeed");
        let path = super::super::utils::log_file_path_in("test", tmp.path())
            .expect("log path should resolve");
        let subscriber = tracing_subscriber::registry().with(layer);
        let guard = tracing::dispatcher::set_default(&tracing::Dispatch::new(subscriber));
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
    fn file_layer_formats_phase_with_phase_tag() {
        let (path, _tmp, _guard) = isolated_file_layer();
        tracing::info!(target: "dotfiles::phase", "Bootstrap");
        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains(":: Bootstrap"),
            "phase should include phase marker: {content}"
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
    fn file_layer_formats_error_tag() {
        let (path, _tmp, _guard) = isolated_file_layer();
        tracing::error!("something broke");
        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("[error] something broke"),
            "error should have [error] tag: {content}"
        );
    }

    #[test]
    fn file_layer_formats_warn_tag() {
        let (path, _tmp, _guard) = isolated_file_layer();
        tracing::warn!("careful now");
        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("[warn] careful now"),
            "warn should have [warn] tag: {content}"
        );
    }

    #[test]
    fn file_layer_formats_debug_without_tag() {
        let (path, _tmp, _guard) = isolated_file_layer();
        tracing::debug!("extra detail");
        let content = fs::read_to_string(&path).unwrap();
        let line = content
            .lines()
            .find(|l| l.contains("extra detail"))
            .unwrap();
        assert!(
            !line.contains("[debug]"),
            "debug should not have [debug] tag: {line}"
        );
    }

    #[test]
    fn file_layer_formats_info_without_special_tag() {
        let (path, _tmp, _guard) = isolated_file_layer();
        tracing::info!("regular info");
        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("regular info"),
            "info message should appear: {content}"
        );
        // Should NOT have any of the special tags
        let info_line = content
            .lines()
            .find(|l| l.contains("regular info"))
            .unwrap();
        assert!(
            !info_line.contains("[error]")
                && !info_line.contains("[warn]")
                && !info_line.contains("[debug]")
                && !info_line.contains("[dry run]")
                && !info_line.contains("==>"),
            "plain info should have no special tag: {info_line}"
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
}
