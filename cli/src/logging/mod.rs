//! Logging infrastructure for structured console and file output.

mod buffered;
mod diagnostic;
mod logger;
mod subscriber;
mod types;
mod utils;

pub use buffered::BufferedLog;
pub use diagnostic::{
    DiagEvent, DiagnosticLog, diag_task_context, diag_thread_name, set_diag_thread_name,
};
pub use logger::Logger;
pub use subscriber::init_subscriber;
pub use types::{Log, Output, TaskEntry, TaskRecorder, TaskStatus};
pub(crate) use utils::dotfiles_cache_dir_readonly;

/// Guard that keeps a test tracing dispatcher installed while holding the
/// process-wide test dispatch lock.
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct TestDispatchGuard {
    _default: tracing::dispatcher::DefaultGuard,
    _lock: TestDispatchLock,
}

/// Guard that serializes tests which exercise tracing callsites.
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct TestDispatchLock {
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
static TEST_DISPATCH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn test_dispatch_lock() -> TestDispatchLock {
    let lock = TEST_DISPATCH_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    TestDispatchLock { _lock: lock }
}

#[cfg(test)]
fn test_dispatch_guard(dispatch: &tracing::Dispatch) -> TestDispatchGuard {
    let lock = test_dispatch_lock();
    let default = tracing::dispatcher::set_default(dispatch);
    TestDispatchGuard {
        _default: default,
        _lock: lock,
    }
}

/// Create a Logger backed by an isolated per-thread tracing subscriber
/// with a [`FileLayer`], so that tracing events emitted by logger methods
/// actually reach the log file during tests.
///
/// Returns a [`TestDispatchGuard`] that must be kept alive for the duration
/// of the test. It restores the previous thread-local dispatcher when
/// dropped and serializes test dispatchers because tracing callsite interest
/// caches are process-global.
#[cfg(test)]
#[allow(clippy::expect_used, reason = "test code uses panicking helpers")]
pub(crate) fn isolated_logger() -> (Logger, tempfile::TempDir, TestDispatchGuard) {
    use tracing_subscriber::layer::SubscriberExt as _;
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let file_layer =
        subscriber::FileLayer::new_in("test", tmp.path()).expect("failed to create file layer");
    let log = Logger::new_in("test", tmp.path());
    let subscriber = tracing_subscriber::registry().with(file_layer);
    let dispatch = tracing::Dispatch::new(subscriber);
    let guard = test_dispatch_guard(&dispatch);
    (log, tmp, guard)
}
