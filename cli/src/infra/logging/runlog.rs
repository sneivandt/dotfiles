//! The run log: a single append-only record of everything that happened.
//!
//! Every message is written here immediately, in true chronological order,
//! regardless of whether it is also shown on the console.  Console output is a
//! separate concern handled by the tracing console layer, so this module never
//! needs to know about verbosity, styling, or terminal state.
mod naming;
mod retention;

pub(crate) use naming::parse_run_log_file_name;
#[cfg(test)]
use naming::run_log_file_name;
pub(crate) use naming::run_log_sort_stamp;
#[cfg(test)]
use retention::RETENTION_LOCK;
use retention::create_run_log_file;
#[cfg(test)]
use retention::lease_path;
#[cfg(test)]
use retention::open_lock;
use retention::prune_run_logs;

use std::cell::RefCell;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use super::records::{Record, RunOutcome, StoredRecord, elapsed_us};
use super::types::LogEvent;
use super::utils::{format_utc_compact, format_utc_datetime_us, strip_ansi};

/// Number of run logs retained in the log directory.
///
/// Old runs are pruned oldest-first at the start of each run, so a failure is
/// still readable long after the run that produced it.
const MAX_RETAINED_RUNS: usize = 50;

static DEGRADATION_WARNING_EMITTED: AtomicBool = AtomicBool::new(false);

#[allow(
    clippy::print_stderr,
    reason = "the console is the only remaining sink when the run log degrades"
)]
pub(super) fn warn_degraded(reason: &str) {
    if !DEGRADATION_WARNING_EMITTED.swap(true, Ordering::Relaxed) {
        eprintln!("warning: persistent run logging is unavailable: {reason}");
    }
}

thread_local! {
    /// Task name for the current thread, set by the parallel scheduler.
    ///
    /// `std::thread::scope` spawns unnamed threads, so
    /// `std::thread::current().name()` returns `None`.  This thread-local
    /// stores the task name so that [`RunLog::emit`] can identify which task
    /// produced each event without needing tracing spans.
    static LOG_TASK_NAME: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Set the run-log task name for the current thread.
///
/// Called by the parallel scheduler when a thread starts working on a task.
/// The name is used by [`RunLog::emit`] as a fallback when the OS thread has
/// no name.
pub fn set_log_thread_name(name: &str) {
    LOG_TASK_NAME.with(|cell| {
        *cell.borrow_mut() = Some(name.to_string());
    });
}

/// Guard that restores the previous run-log task context when dropped.
#[derive(Debug)]
pub struct LogTaskContextGuard {
    previous: Option<String>,
}

impl Drop for LogTaskContextGuard {
    fn drop(&mut self) {
        LOG_TASK_NAME.with(|cell| {
            *cell.borrow_mut() = self.previous.take();
        });
    }
}

/// Set the run-log task context for the current scope.
#[must_use]
pub fn log_task_context(name: &str) -> LogTaskContextGuard {
    let previous = LOG_TASK_NAME.with(|cell| cell.borrow_mut().replace(name.to_string()));
    LogTaskContextGuard { previous }
}

/// Read the run-log task name for the current thread.
#[must_use]
pub fn log_thread_name() -> String {
    if let Some(name) = LOG_TASK_NAME.with(|cell| cell.borrow().clone()) {
        return name;
    }
    let thread = std::thread::current();
    if let Some(name) = thread.name() {
        return name.to_string();
    }
    "?".to_string()
}

fn needs_structured_context(context: &str) -> bool {
    context.contains([']', '\r', '\n', '\0'])
}

/// The run log: every event, in the order it actually happened.
///
/// Written to one file per run in the dotfiles log directory
/// (`%LOCALAPPDATA%\dotfiles\logs` on Windows, `$XDG_STATE_HOME/dotfiles/logs`
/// elsewhere).  Each line records a sequence number, microsecond-precision
/// elapsed time from program start, a wall-clock timestamp, the originating
/// task/thread context, and the event kind, which together make it possible to
/// reconstruct the true interleaved timeline of parallel execution.
///
/// Entries are written immediately as they are produced.  This is deliberately
/// decoupled from console rendering: the console buffers per-task output to
/// keep parallel runs readable, whereas the run log always reflects real
/// chronological order.
#[derive(Debug)]
pub struct RunLog {
    file: Mutex<fs::File>,
    lease: fs::File,
    path: PathBuf,
    start: Instant,
    sequence: AtomicU64,
    healthy: AtomicBool,
    finished: AtomicBool,
}

impl RunLog {
    /// Create the run log for `command` in the resolved dotfiles log
    /// directory.
    ///
    /// Returns `None` if the log directory or file cannot be created, in
    /// which case the run simply proceeds without a file log.
    pub(super) fn create(command: &str, start: Instant) -> Option<Self> {
        Self::new(command, &super::utils::dotfiles_log_dir()?, start)
    }

    /// Create a new run log file for the given command.
    ///
    /// `log_dir` is the resolved dotfiles log directory.  The caller is
    /// responsible for resolving the directory; this constructor never reads
    /// environment variables.
    ///
    /// Returns `None` if the file cannot be created.
    pub(super) fn new(command: &str, log_dir: &Path, start: Instant) -> Option<Self> {
        Self::new_retaining(command, log_dir, start, MAX_RETAINED_RUNS)
    }

    /// Like [`new`](Self::new) but with an explicit retention count, so tests
    /// can exercise pruning without writing dozens of files.
    fn new_retaining(command: &str, log_dir: &Path, start: Instant, keep: usize) -> Option<Self> {
        let (path, mut file, lease) = create_run_log_file(command, log_dir, &format_utc_compact())?;
        let version =
            option_env!("DOTFILES_VERSION").unwrap_or(concat!("dev-", env!("CARGO_PKG_VERSION")));
        let header = format!(
            "# Dotfiles {version} {}\n\
             # Columns: seq | elapsed_us | wall_utc | context | event | message\n",
            format_utc_datetime_us(),
        );
        file.write_all(header.as_bytes()).ok()?;
        // Prune after the new file exists so a crashed run still leaves a log
        // behind, and so pruning stays off the end-of-run critical path.
        prune_run_logs(log_dir, keep);
        Some(Self {
            file: Mutex::new(file),
            lease,
            path,
            start,
            sequence: AtomicU64::new(0),
            healthy: AtomicBool::new(true),
            finished: AtomicBool::new(false),
        })
    }

    /// Stable filename stem, also accepted by `dotfiles log --id`.
    pub(crate) fn id(&self) -> String {
        self.path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }

    pub(crate) fn start_run(&self, command: &str, parent_run_id: Option<String>) {
        self.record(Record::RunStart {
            run_id: self.id(),
            command: command.into(),
            parent_run_id,
        });
    }

    pub(crate) fn finish(&self, outcome: RunOutcome, exit_code: i32) {
        if !self.finished.swap(true, Ordering::Relaxed) {
            self.record(Record::RunFinish {
                outcome,
                elapsed_us: elapsed_us(self.start.elapsed()),
                exit_code,
            });
        }
    }

    pub(crate) fn record_action(&self, verb: &str, subject: &str, planned: bool, message: &str) {
        self.record(Record::Action {
            verb: verb.into(),
            subject: subject.into(),
            planned,
            message: message.into(),
        });
    }

    pub(crate) fn record(&self, record: Record) {
        self.record_in_context(&log_thread_name(), record);
    }

    pub(crate) fn record_in_context(&self, context: &str, record: Record) {
        match serde_json::to_string(&StoredRecord::new(context, record)) {
            Ok(message) => self.write_encoded_record(context, &message),
            Err(error) => self.mark_degraded(&format!("encoding record failed: {error}")),
        }
    }

    pub(crate) fn emit_encoded_record(&self, json: &str) {
        let trimmed = json.trim();
        if trimmed.is_empty() {
            return;
        }
        self.write_encoded_record(&log_thread_name(), trimmed);
    }

    fn write_encoded_record(&self, context: &str, json: &str) {
        // The JSON envelope retains identities that cannot fit the text header.
        let header_context = if needs_structured_context(context) {
            "record-context"
        } else {
            context
        };
        self.write_event(LogEvent::Record, header_context, json);
    }

    /// Emit an event, attributing it to the current thread's task context.
    ///
    /// Each line is:
    /// `<seq> +<elapsed_us> <wall_utc_us> [<context>] [<event>] <message>`
    ///
    /// ANSI escape sequences are stripped from the message. The context comes
    /// from the current task context when one is set, otherwise from the OS
    /// thread name when available (e.g. `"main"`). Blank messages are omitted.
    pub fn emit(&self, event: LogEvent, message: &str) {
        self.emit_task(event, &log_thread_name(), message);
    }

    /// Emit an event with an explicit task context, preserving multiline text.
    pub fn emit_task(&self, event: LogEvent, context: &str, message: &str) {
        let multiline = message.contains(['\n', '\r']);
        let clean = strip_ansi(message);
        if !multiline && clean.trim().is_empty() {
            return;
        }
        if multiline || clean.contains('\0') || needs_structured_context(context) {
            self.record_in_context(
                context,
                Record::Message {
                    event: event.name().into(),
                    text: if multiline {
                        clean
                    } else {
                        clean.trim().to_owned()
                    },
                },
            );
        } else {
            self.write_event(event, context, clean.trim());
        }
    }

    fn write_event(&self, event: LogEvent, context: &str, formatted_message: &str) {
        let Ok(mut f) = self.file.lock() else {
            self.mark_degraded("the log file lock was poisoned");
            return;
        };
        let seq = self
            .sequence
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let elapsed = self.start.elapsed();
        let elapsed_us = elapsed.as_micros();
        let wall = format_utc_datetime_us();
        let event_name = event.name();
        let line = format!(
            "{seq:06} +{elapsed_us:>12} {wall} [{context}] [{event_name}] {formatted_message}\n"
        );
        if let Err(error) = f.write_all(line.as_bytes()) {
            self.mark_degraded(&format!("write failed: {error}"));
        }
    }

    /// Whether the persistent sink has remained writable.
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }

    fn mark_degraded(&self, reason: &str) {
        self.healthy.store(false, Ordering::Relaxed);
        warn_degraded(reason);
    }

    /// Return the path of the run log file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for RunLog {
    fn drop(&mut self) {
        // Closing only this handle can leave an inherited/duplicated open-file
        // description locked. The logger owns the lease, not its subprocesses.
        if let Err(error) = self.lease.unlock() {
            tracing::debug!("could not release finished run-log lease: {error}");
        }
    }
}

#[cfg(test)]
mod tests;
