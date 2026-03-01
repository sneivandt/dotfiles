//! Tracing subscriber setup: console formatter, file layer, and initialisation.
use std::fs;
use std::io::Write as _;
use std::sync::Mutex;

use super::utils::{format_utc_datetime, format_utc_time, log_file_path, strip_ansi};

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
        let version =
            option_env!("DOTFILES_VERSION").unwrap_or(concat!("dev-", env!("CARGO_PKG_VERSION")));
        let header = format!(
            "==========================================\n\
             Dotfiles {version} {}\n\
             ==========================================\n",
            format_utc_datetime(),
        );
        fs::write(&path, header).ok()?;
        let file = fs::OpenOptions::new().append(true).open(&path).ok()?;
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
            (tracing::Level::INFO, "dotfiles::stage") => format!("[{ts}] ==> {msg}"),
            (tracing::Level::INFO, "dotfiles::dry_run") => format!("[{ts}]     [dry run] {msg}"),
            (tracing::Level::ERROR, _) => format!("[{ts}]     [error] {msg}"),
            (tracing::Level::WARN, _) => format!("[{ts}]     [warn] {msg}"),
            (tracing::Level::DEBUG, _) => format!("[{ts}]     [debug] {msg}"),
            _ => format!("[{ts}]     {msg}"),
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
            tracing::Level::INFO if target == "dotfiles::stage" => {
                writeln!(writer, "\x1b[1;34m==>\x1b[0m \x1b[1m{msg}\x1b[0m")
            }
            tracing::Level::INFO if target == "dotfiles::dry_run" => {
                writeln!(writer, "  \x1b[33m[DRY RUN]\x1b[0m {msg}")
            }
            tracing::Level::INFO => writeln!(writer, "  {msg}"),
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
