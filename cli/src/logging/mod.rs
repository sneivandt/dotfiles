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

/// Create a production-topology subscriber for regression tests.
///
/// Returns a `Logger`, a temp directory, a guard that restores the previous
/// dispatcher on drop, and a `Arc<Mutex<Vec<u8>>>` that receives the bytes
/// written by the INFO-level "console" layer.
///
/// The topology mirrors [`crate::logging::subscriber::init_subscriber`]:
/// - Console layer at `INFO` (writes to the returned buffer)
/// - File layer at `DEBUG` (writes to a temp file)
///
/// This two-layer setup is required to reproduce the `tracing::enabled!`
/// `FilterState` corruption bug: with a single DEBUG layer, all events are
/// always accepted and the stale-bits problem never manifests.
#[cfg(test)]
#[allow(clippy::expect_used)]
pub(crate) fn two_layer_logger() -> (
    Logger,
    tempfile::TempDir,
    tracing::dispatcher::DefaultGuard,
    std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
) {
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::{Layer as _, filter::LevelFilter, fmt, layer::SubscriberExt as _};

    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let file_layer =
        subscriber::FileLayer::new_in("test", tmp.path()).expect("failed to create file layer");
    let log = Logger::new_in("test", tmp.path());

    let console_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let buf_for_writer = Arc::clone(&console_buf);

    let console_layer = fmt::layer()
        .with_writer(move || -> Box<dyn std::io::Write> {
            Box::new(VecWriter(Arc::clone(&buf_for_writer)))
        })
        .with_filter(LevelFilter::INFO);

    let subscriber = tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer.with_filter(LevelFilter::DEBUG));
    let guard = tracing::dispatcher::set_default(&tracing::Dispatch::new(subscriber));
    (log, tmp, guard, console_buf)
}

/// A `Write` impl that appends to a shared `Arc<Mutex<Vec<u8>>>`.
///
/// Used by the INFO-level console capture layer in [`two_layer_logger`].
#[cfg(test)]
pub(crate) struct VecWriter(pub(crate) std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

#[cfg(test)]
impl std::io::Write for VecWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
