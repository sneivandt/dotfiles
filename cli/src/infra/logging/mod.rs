//! Logging infrastructure for structured console and file output.
//!
//! Two independent sinks share one set of messages:
//!
//! - the **run log** ([`RunLog`]), an append-only file that records every
//!   event immediately in true chronological order;
//! - the **console**, rendered by the tracing console layer, which buffers
//!   per-task output so parallel runs stay readable.
//!
//! `Logger` writes to the run log directly and emits `dotfiles::ui::*` tracing
//! events for the console. Raw `tracing` calls elsewhere in the crate reach the
//! run log through [`subscriber::RunLogLayer`].

mod buffered;
mod logger;
mod runlog;
mod style;
mod subscriber;
mod types;
mod utils;

pub use buffered::BufferedLog;
pub use logger::Logger;
pub use runlog::{LogEvent, log_task_context, log_thread_name, set_log_thread_name};
pub use types::{ActionCounts, Log, MsgKind, Output, OutputExt, TaskStatus, TaskVisibility};
// Only the in-crate unit tests reach `TaskRecorder` from outside the `logging`
// module; production code uses the `super::types` path directly.
pub(crate) use runlog::parse_run_log_file_name;
#[cfg(test)]
pub use types::TaskRecorder;
pub(crate) use utils::{dotfiles_log_dir_readonly, format_elapsed};

/// Initialise logging for a command run.
///
/// Opens the run log, installs the global tracing subscriber (console layer
/// plus the run-log bridge), and returns the [`Logger`] that shares the same
/// run log. Must be called once at program startup, before any logging.
#[must_use]
pub fn init(verbose: bool, symbols: bool, command: &str) -> Logger {
    let mut log = Logger::new(command);
    log.set_verbose(verbose);
    log.set_symbols(symbols);
    subscriber::init_subscriber(verbose, log.run_log_handle());
    log
}

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
/// wired to the same run log, so that tracing events emitted by logger methods
/// actually reach the log file during tests.
///
/// Returns a [`TestDispatchGuard`] that must be kept alive for the duration
/// of the test. It restores the previous thread-local dispatcher when
/// dropped and serializes test dispatchers because tracing callsite interest
/// caches are process-global.
#[cfg(test)]
#[allow(clippy::expect_used, reason = "test code uses panicking helpers")]
pub(crate) fn isolated_logger() -> (Logger, tempfile::TempDir, TestDispatchGuard) {
    isolated_logger_for("test")
}

/// Like [`isolated_logger`] but for an explicit command name, so tests can
/// exercise command-dependent summary formatting.
#[cfg(test)]
#[allow(clippy::expect_used, reason = "test code uses panicking helpers")]
pub(crate) fn isolated_logger_for(command: &str) -> (Logger, tempfile::TempDir, TestDispatchGuard) {
    use tracing_subscriber::layer::SubscriberExt as _;
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let log = Logger::new_in(command, tmp.path());
    let run_log_layer =
        subscriber::RunLogLayer::new(log.run_log_handle().expect("run log should be created"));
    let subscriber = tracing_subscriber::registry().with(run_log_layer);
    let dispatch = tracing::Dispatch::new(subscriber);
    let guard = test_dispatch_guard(&dispatch);
    (log, tmp, guard)
}
