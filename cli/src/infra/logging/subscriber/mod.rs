//! Tracing subscriber setup and initialization.

mod console;
mod event;
mod run_log;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use console::DotfilesFormatter;
pub(in crate::infra::logging) use console::{
    emit_console, emit_task_result, set_transient_progress, set_verbose,
    take_transient_progress_rows, transient_progress_rows,
};
pub(in crate::infra::logging) use run_log::RunLogLayer;

use super::runlog::RunLog;

/// Initialise the global [`tracing`] subscriber.
///
/// Installs the console layer for raw `tracing` diagnostics, plus a bridge that
/// records those events from `infra` and `domains` into `run_log`. `Logger`
/// renders its own user-facing messages directly.
///
/// Must be called once at program startup, before any logging.
pub(in crate::infra::logging) fn init_subscriber(verbose: bool, run_log: Option<Arc<RunLog>>) {
    use tracing_subscriber::fmt::writer::MakeWriterExt as _;
    use tracing_subscriber::{
        Layer as _, filter::LevelFilter, fmt, layer::SubscriberExt as _,
        util::SubscriberInitExt as _,
    };

    set_verbose(verbose);

    let make_writer = std::io::stderr
        .with_max_level(tracing::Level::WARN)
        .and(std::io::stdout.with_min_level(tracing::Level::INFO));

    let console_layer = fmt::layer()
        .event_format(DotfilesFormatter)
        .with_writer(make_writer)
        .with_filter(if verbose {
            // Verbose lets raw diagnostic information reach the formatter.
            LevelFilter::DEBUG
        } else {
            LevelFilter::INFO
        });

    let run_log_layer = run_log.map(|log| RunLogLayer::new(log).with_filter(LevelFilter::DEBUG));

    tracing_subscriber::registry()
        .with(console_layer)
        .with(run_log_layer)
        .init();
}
