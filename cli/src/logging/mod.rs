//! Logging infrastructure for structured console and file output.

mod buffered;
mod diagnostic;
mod logger;
mod subscriber;
mod types;
mod utils;

pub use buffered::BufferedLog;
pub use diagnostic::{DiagEvent, DiagnosticLog, diag_thread_name, set_diag_thread_name};
pub use logger::Logger;
pub use subscriber::init_subscriber;
pub use types::{Log, Output, TaskEntry, TaskRecorder, TaskStatus};

/// Create a Logger backed by an isolated per-thread tracing subscriber
/// with a [`FileLayer`], so that tracing events emitted by logger methods
/// actually reach the log file during tests.
///
/// Returns a [`tracing::dispatcher::DefaultGuard`] that must be kept alive
/// for the duration of the test — dropping it restores the previous
/// thread-local dispatcher.
#[cfg(test)]
#[allow(clippy::expect_used)]
pub(crate) fn isolated_logger() -> (Logger, tempfile::TempDir, tracing::dispatcher::DefaultGuard) {
    use tracing_subscriber::{Layer as _, filter::LevelFilter, layer::SubscriberExt as _};
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let file_layer =
        subscriber::FileLayer::new_in("test", tmp.path()).expect("failed to create file layer");
    let log = Logger::new_in("test", tmp.path());
    let subscriber =
        tracing_subscriber::registry().with(file_layer.with_filter(LevelFilter::DEBUG));
    let guard = tracing::dispatcher::set_default(&tracing::Dispatch::new(subscriber));
    (log, tmp, guard)
}
