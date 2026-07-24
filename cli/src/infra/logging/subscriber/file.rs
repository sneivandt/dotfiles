//! Persistent tracing file output.

use std::fs;
use std::io::Write as _;
use std::sync::Mutex;

use super::event::MessageExtractor;
use crate::infra::logging::utils::{
    format_utc_datetime, format_utc_time, log_file_path, strip_ansi,
};

/// Task context stored on tracing spans created by task execution.
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
                .and_then(|task| task.strip_suffix('"'))
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
/// Created by `init_subscriber` so that file output goes through the same
/// tracing pipeline as console output. Always captures events at `DEBUG`
/// level and above regardless of the console verbosity setting.
#[derive(Debug)]
pub(in crate::infra::logging) struct FileLayer {
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
    pub(in crate::infra::logging) fn new_in(
        command: &str,
        cache_dir: &std::path::Path,
    ) -> Option<Self> {
        let path = crate::infra::logging::utils::log_file_path_in(command, cache_dir)?;
        Self::create_at(&path)
    }

    /// Shared implementation: write a header and open the file for appending.
    #[allow(clippy::print_stderr, reason = "intentional user-facing output")]
    fn create_at(path: &std::path::Path) -> Option<Self> {
        let version =
            option_env!("DOTFILES_VERSION").unwrap_or(concat!("dev-", env!("CARGO_PKG_VERSION")));
        let header = format!(
            "==========================================\n\
             Dotfiles {version} {}\n\
             ==========================================\n",
            format_utc_datetime(),
        );
        let mut file = match fs::File::create(path) {
            Ok(file) => file,
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
        let timestamp = format_utc_time();
        let context = task_name.map_or_else(String::new, |name| format!(" [{name}]"));
        let level_label = log_level_label(level, target);
        let prefix = format!("[{timestamp}]{context} [{level_label}]");

        let line = match (level, target) {
            (
                tracing::Level::INFO,
                "dotfiles::stage"
                | "dotfiles::task_stage"
                | "dotfiles::file_only_stage"
                | "dotfiles::file_only_task_stage",
            ) => {
                format!("{prefix} ==> {msg}")
            }
            _ => format!("{prefix} {msg}"),
        };

        if let Ok(mut file) = self.file.lock() {
            drop(writeln!(file, "{line}"));
        }
    }
}

fn log_level_label(level: tracing::Level, target: &str) -> &'static str {
    match (level, target) {
        (tracing::Level::INFO, "dotfiles::file_only_error") | (tracing::Level::ERROR, _) => "error",
        (tracing::Level::INFO, "dotfiles::file_only_warn") | (tracing::Level::WARN, _) => "warn",
        (tracing::Level::INFO, "dotfiles::file_only_debug") | (tracing::Level::DEBUG, _) => "debug",
        (
            tracing::Level::INFO,
            "dotfiles::stage"
            | "dotfiles::task_stage"
            | "dotfiles::file_only_stage"
            | "dotfiles::file_only_task_stage",
        ) => "stage",
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
