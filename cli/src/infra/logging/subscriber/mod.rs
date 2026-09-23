//! Tracing subscriber setup and initialization.

mod console;
mod event;
mod run_log;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use super::{console::Console, runlog::RunLog};
pub(in crate::infra::logging) use console::ConsoleLayer;
pub(in crate::infra::logging) use run_log::RunLogLayer;

/// Capture raw diagnostics in the same console and run log used by `Logger`.
pub(in crate::infra::logging) fn init_subscriber(
    console: Arc<Console>,
    run_log: Option<Arc<RunLog>>,
) {
    use tracing_subscriber::{
        Layer as _, filter::LevelFilter, layer::SubscriberExt as _, util::SubscriberInitExt as _,
    };
    let console_layer = ConsoleLayer::new(console).with_filter(LevelFilter::INFO);
    let run_log_layer = run_log.map(|log| RunLogLayer::new(log).with_filter(LevelFilter::DEBUG));
    tracing_subscriber::registry()
        .with(console_layer)
        .with(run_log_layer)
        .init();
}
