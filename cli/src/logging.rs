//! Logging infrastructure for structured console and file output.
use std::cell::RefCell;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

thread_local! {
    /// Task name for the current thread, set by the parallel scheduler.
    ///
    /// `std::thread::scope` spawns unnamed threads, so
    /// `std::thread::current().name()` returns `None`.  This thread-local
    /// stores the task name so that [`DiagnosticLog::emit`] can identify
    /// which task produced each event.
    static DIAG_TASK_NAME: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Set the diagnostic task name for the current thread.
///
/// Called by the parallel scheduler when a thread starts working on a task.
/// The name is used by [`DiagnosticLog::emit`] as a fallback when the OS
/// thread has no name.
pub fn set_diag_thread_name(name: &str) {
    DIAG_TASK_NAME.with(|cell| {
        *cell.borrow_mut() = Some(name.to_string());
    });
}

/// Read the diagnostic task name for the current thread.
pub(crate) fn diag_thread_name() -> String {
    // Prefer the OS thread name ("main"), fall back to the task-local name.
    let thread = std::thread::current();
    if let Some(name) = thread.name() {
        return name.to_string();
    }
    DIAG_TASK_NAME.with(|cell| cell.borrow().as_deref().unwrap_or("?").to_string())
}

/// Task execution result for summary reporting.
#[derive(Debug, Clone)]
pub struct TaskEntry {
    /// Human-readable task name.
    pub name: String,
    /// Final status of the task.
    pub status: TaskStatus,
    /// Optional detail message (e.g., skip reason or error description).
    pub message: Option<String>,
}

/// Status of a completed task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task completed successfully.
    Ok,
    /// Task was skipped because it does not apply to the current platform or profile.
    NotApplicable,
    /// Task was explicitly skipped (e.g., tool not found, config empty).
    Skipped,
    /// Task ran in dry-run mode; no changes were applied.
    DryRun,
    /// Task encountered an error and could not complete.
    Failed,
}

/// Abstraction over logging backends.
///
/// Both [`Logger`] (direct output) and [`BufferedLog`] (deferred output for
/// parallel tasks) implement this trait, allowing task code to log without
/// knowing whether output is immediate or buffered.
pub trait Log: Send + Sync {
    /// Log a stage header (major section).
    fn stage(&self, msg: &str);
    /// Log an informational message.
    fn info(&self, msg: &str);
    /// Log a debug message (may be suppressed on console).
    fn debug(&self, msg: &str);
    /// Log a warning message.
    fn warn(&self, msg: &str);
    /// Log an error message.
    fn error(&self, msg: &str);
    /// Log a dry-run action message.
    fn dry_run(&self, msg: &str);
    /// Record a task result for the summary.
    fn record_task(&self, name: &str, status: TaskStatus, message: Option<&str>);
    /// Access the high-precision diagnostic log, if available.
    fn diagnostic(&self) -> Option<&DiagnosticLog> {
        None
    }
}

/// A single buffered log entry, replayed when flushed.
#[derive(Debug, Clone)]
enum LogEntry {
    Stage(String),
    Info(String),
    Debug(String),
    Warn(String),
    Error(String),
    DryRun(String),
}

impl LogEntry {
    /// Replay this entry to the console and log file via tracing.
    ///
    /// Does **not** write to the diagnostic log because the entry was
    /// already recorded there in real-time when it was buffered.
    fn replay(&self) {
        match self {
            Self::Stage(msg) => tracing::info!(target: "dotfiles::stage", "{msg}"),
            Self::Info(msg) => tracing::info!("{msg}"),
            Self::Debug(msg) => tracing::debug!("{msg}"),
            Self::Warn(msg) => tracing::warn!("{msg}"),
            Self::Error(msg) => tracing::error!("{msg}"),
            Self::DryRun(msg) => tracing::info!(target: "dotfiles::dry_run", "{msg}"),
        }
    }
}

/// Implement the display methods of [`Log`] by delegating to inherent methods
/// of the same name on the implementing type.
///
/// The `record_task` method is **not** included because its signature differs
/// from the `fn(&self, &str)` pattern shared by the display methods.
macro_rules! forward_log_methods {
    ($($method:ident),+ $(,)?) => {
        $(
            fn $method(&self, msg: &str) {
                self.$method(msg);
            }
        )+
    };
}

/// Implement the display methods of [`Log`] by buffering each message into
/// `self.entries` as the corresponding [`LogEntry`] variant.
///
/// Each method also forwards the message to the diagnostic log in real-time
/// (bypassing the buffer) so that the true chronological order of events
/// during parallel execution is preserved.
///
/// The `record_task` method is **not** included because it forwards to
/// `self.inner` instead of buffering.
macro_rules! buffer_log_methods {
    ($($method:ident => $variant:ident => $diag:ident),+ $(,)?) => {
        $(
            fn $method(&self, msg: &str) {
                // Write to diagnostic log immediately (real-time timestamp).
                if let Some(d) = &self.inner.diagnostic {
                    d.emit(DiagEvent::$diag, msg);
                }
                if let Ok(mut guard) = self.entries.lock() {
                    guard.push(LogEntry::$variant(msg.to_string()));
                }
            }
        )+
    };
}

/// Buffered logger for parallel task execution.
///
/// Captures display output (stage, info, debug, etc.) in memory so that
/// parallel tasks do not interleave their console output.  The captured
/// entries are replayed in order when `flush_and_complete` is called.
///
/// [`record_task`](Log::record_task) is forwarded directly to the underlying
/// [`Logger`] because the summary collection is already thread-safe.
#[derive(Debug)]
pub struct BufferedLog {
    inner: Arc<Logger>,
    entries: Mutex<Vec<LogEntry>>,
}

impl BufferedLog {
    /// Create a new buffered logger backed by the given [`Logger`].
    #[must_use]
    pub const fn new(inner: Arc<Logger>) -> Self {
        Self {
            inner,
            entries: Mutex::new(Vec::new()),
        }
    }

    /// Replay all buffered entries to the backing [`Logger`].
    #[cfg(test)]
    pub fn flush(&self) {
        let entries = match self.entries.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        for entry in &entries {
            entry.replay();
        }
    }

    /// Flush all buffered entries and remove the task from the active set.
    ///
    /// Acquires the flush lock on the backing [`Logger`] to prevent
    /// interleaved console output when multiple tasks complete concurrently.
    /// After replaying the buffered entries, updates the active task display.
    pub fn flush_and_complete(&self, task_name: &str) {
        let _guard = self
            .inner
            .flush_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.inner.clear_progress();
        let entries = match self.entries.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        for entry in &entries {
            entry.replay();
        }
        let remaining = self.inner.active_tasks.lock().ok().and_then(|mut active| {
            active.retain(|n| n != task_name);
            (!active.is_empty()).then(|| active.join(", "))
        });
        if let Some(names) = remaining {
            self.inner.draw_progress(&names);
        }
    }
}

impl Log for BufferedLog {
    buffer_log_methods! {
        stage   => Stage   => Stage,
        info    => Info    => Info,
        debug   => Debug   => Debug,
        warn    => Warn    => Warn,
        error   => Error   => Error,
        dry_run => DryRun  => DryRun,
    }

    fn record_task(&self, name: &str, status: TaskStatus, message: Option<&str>) {
        // Forward directly — the Logger's task list is already Mutex-protected.
        self.inner.record_task(name, status, message);
    }

    fn diagnostic(&self) -> Option<&DiagnosticLog> {
        self.inner.diagnostic.as_ref()
    }
}

/// High-precision diagnostic log for capturing the real-time sequence of events.
///
/// Unlike the main log file (which replays buffered output per-task and uses
/// second-precision timestamps), the diagnostic log writes every event
/// **immediately** with microsecond-precision elapsed time from program start,
/// the originating thread name, and an event kind tag.  This makes it possible
/// to reconstruct the true interleaved timeline of parallel execution.
///
/// Written to `$XDG_CACHE_HOME/dotfiles/<command>.diag.log`.
#[derive(Debug)]
pub struct DiagnosticLog {
    file: Mutex<fs::File>,
    #[cfg_attr(not(test), allow(dead_code))]
    path: PathBuf,
    start: Instant,
}

/// Event kinds for the diagnostic log.
///
/// Each variant maps to a short uppercase tag in the log output.
#[derive(Debug, Clone, Copy)]
pub enum DiagEvent {
    /// Informational message from a task.
    Info,
    /// Debug-level message.
    Debug,
    /// Warning message.
    Warn,
    /// Error message.
    Error,
    /// Stage header (major section).
    Stage,
    /// Dry-run preview.
    DryRun,
    /// A task thread has been spawned and is waiting for dependencies.
    TaskWait,
    /// A task's dependencies are satisfied; execution begins.
    TaskStart,
    /// A task finished executing.
    TaskDone,
    /// A task was skipped (not applicable).
    TaskSkip,
    /// Resource state check.
    ResourceCheck,
    /// Resource apply (mutation).
    ResourceApply,
    /// Resource apply result.
    ResourceResult,
    /// Resource removal.
    ResourceRemove,
}

impl DiagEvent {
    /// Short tag for the log line.
    const fn tag(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
            Self::Stage => "STAGE",
            Self::DryRun => "DRYRUN",
            Self::TaskWait => "TASK_WAIT",
            Self::TaskStart => "TASK_START",
            Self::TaskDone => "TASK_DONE",
            Self::TaskSkip => "TASK_SKIP",
            Self::ResourceCheck => "RES_CHECK",
            Self::ResourceApply => "RES_APPLY",
            Self::ResourceResult => "RES_RESULT",
            Self::ResourceRemove => "RES_REMOVE",
        }
    }
}

impl DiagnosticLog {
    /// Create a new diagnostic log file for the given command.
    ///
    /// Returns `None` if the cache directory cannot be created or the file
    /// cannot be opened.
    fn new(command: &str, start: Instant) -> Option<Self> {
        let path = diag_log_file_path(command)?;
        let version =
            option_env!("DOTFILES_VERSION").unwrap_or(concat!("dev-", env!("CARGO_PKG_VERSION")));
        let header = format!(
            "# Diagnostic log — dotfiles {version} {}\n\
             # Columns: elapsed_us | wall_utc | thread | event | message\n",
            format_utc_datetime_us(),
        );
        fs::write(&path, header).ok()?;
        let file = fs::OpenOptions::new().append(true).open(&path).ok()?;
        Some(Self {
            file: Mutex::new(file),
            path,
            start,
        })
    }

    /// Emit a diagnostic event.
    ///
    /// Each line is: `+<elapsed_us> <wall_utc_us> [<thread>] <TAG> <message>`
    ///
    /// ANSI escape sequences are stripped from the message.  The thread
    /// identifier comes from the OS thread name when available (e.g.
    /// `"main"`), otherwise from the task name set via
    /// [`set_diag_thread_name`].
    pub fn emit(&self, event: DiagEvent, message: &str) {
        let elapsed = self.start.elapsed();
        let elapsed_us = elapsed.as_micros();
        let wall = format_utc_datetime_us();
        let thread_name = diag_thread_name();
        let tag = event.tag();
        let clean = strip_ansi(message);
        let line = format!("+{elapsed_us:>12} {wall} [{thread_name}] {tag:<12} {clean}\n");
        if let Ok(mut f) = self.file.lock() {
            // Best-effort write — diagnostic logging is non-critical and must
            // never disrupt normal execution (e.g. on a full disk).
            f.write_all(line.as_bytes()).ok();
        }
    }

    /// Emit a diagnostic event with an explicit task name context.
    pub fn emit_task(&self, event: DiagEvent, task: &str, message: &str) {
        if message.is_empty() {
            self.emit(event, &format!("[{task}]"));
        } else {
            self.emit(event, &format!("[{task}] {message}"));
        }
    }
}

/// Return the diagnostic log file path under `$XDG_CACHE_HOME/dotfiles/`.
fn diag_log_file_path(command: &str) -> Option<PathBuf> {
    Some(dotfiles_cache_dir()?.join(format!("{command}.diag.log")))
}

/// Format the current UTC time as `YYYY-MM-DDTHH:MM:SS.ffffffZ` (microsecond precision).
fn format_utc_datetime_us() -> String {
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let (y, mo, d, h, mi, s) = secs_to_ymd_hms(dur.as_secs());
    let us = dur.subsec_micros();
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{us:06}Z")
}

/// Structured logger with dry-run awareness and summary collection.
///
/// All messages are always written to a persistent log file at
/// `$XDG_CACHE_HOME/dotfiles/<command>.log` (default `~/.cache/dotfiles/<command>.log`)
/// with timestamps and ANSI codes stripped, regardless of the verbose flag.
#[derive(Debug)]
pub struct Logger {
    tasks: Mutex<Vec<TaskEntry>>,
    log_file: Option<PathBuf>,
    /// Serializes console output from parallel task flushes.
    flush_lock: Mutex<()>,
    /// Names of tasks currently executing in parallel.
    active_tasks: Mutex<Vec<String>>,
    /// Whether a progress line is currently displayed (`0` = no, `1` = yes).
    ///
    /// The progress line is always truncated to fit within a single terminal
    /// row, so the only valid values are `0` and `1`.  This avoids multi-row
    /// cursor arithmetic that can erase real output when the terminal width
    /// differs from the `COLUMNS` environment variable.
    progress_rows: Mutex<u16>,
    /// High-precision diagnostic log; `None` when the cache dir is unavailable.
    diagnostic: Option<DiagnosticLog>,
}

/// Return the `$XDG_CACHE_HOME/dotfiles/` directory, creating it if needed.
fn dotfiles_cache_dir() -> Option<PathBuf> {
    let cache_dir = std::env::var("XDG_CACHE_HOME").map_or_else(
        |_| {
            std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .map_or_else(|_| PathBuf::from("."), PathBuf::from)
                .join(".cache")
        },
        PathBuf::from,
    );
    let dir = cache_dir.join("dotfiles");
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Return the log file path under `$XDG_CACHE_HOME/dotfiles/` (or `~/.cache/dotfiles/`).
fn log_file_path(command: &str) -> Option<PathBuf> {
    Some(dotfiles_cache_dir()?.join(format!("{command}.log")))
}

/// Return the current UTC time as seconds since the Unix epoch.
fn current_utc_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Convert days since the Unix epoch to `(year, month, day)` in the proleptic
/// Gregorian calendar.  Algorithm from Howard Hinnant's *date algorithms*.
const fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Decompose seconds since the Unix epoch into `(year, month, day, hour, minute, second)`.
const fn secs_to_ymd_hms(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let (y, mo, d) = days_to_ymd(secs / 86400);
    let day_secs = secs % 86400;
    let h = day_secs / 3600;
    let mi = (day_secs % 3600) / 60;
    let s = day_secs % 60;
    (y, mo, d, h, mi, s)
}

/// Format the current UTC time as `YYYY-MM-DD HH:MM:SS`.
fn format_utc_datetime() -> String {
    let (y, mo, d, h, mi, s) = secs_to_ymd_hms(current_utc_secs());
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

/// Format the current UTC time as `HH:MM:SS`.
fn format_utc_time() -> String {
    let (_, _, _, h, mi, s) = secs_to_ymd_hms(current_utc_secs());
    format!("{h:02}:{mi:02}:{s:02}")
}

/// Return the terminal width in columns.
///
/// Reads the `COLUMNS` environment variable, falling back to 80 if unset
/// or unparseable.
fn terminal_columns() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(80)
}

/// Strip ANSI escape sequences from a string.
///
/// Handles SGR sequences (ending in `m`) and other CSI sequences (ending
/// in any letter in the `@`..`~` range), so cursor movement, erase, etc.
/// are also stripped without consuming unrelated text.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip CSI sequences: ESC followed by `[` then parameters ending
            // in a letter in the range 0x40..0x7E (@A-Z[\]^_`a-z{|}~).
            // Also handle non-CSI escapes (e.g., ESC + single char).
            if let Some(next) = chars.next()
                && next == '['
            {
                // CSI sequence: consume until a final byte in '@'..='~'
                for inner in chars.by_ref() {
                    if ('@'..='~').contains(&inner) {
                        break;
                    }
                }
            }
            // else: two-char escape (e.g., ESC M) — already consumed
        } else {
            out.push(c);
        }
    }
    out
}

impl Logger {
    /// Create a new logger.
    ///
    /// Stores the log file path for display in the run summary.  The log file
    /// itself is created and initialised by [`init_subscriber`] via
    /// [`FileLayer`]; this constructor does not write to the file.
    #[must_use]
    pub fn new(command: &str) -> Self {
        let start = Instant::now();
        Self {
            tasks: Mutex::new(Vec::new()),
            log_file: log_file_path(command),
            flush_lock: Mutex::new(()),
            active_tasks: Mutex::new(Vec::new()),
            progress_rows: Mutex::new(0),
            diagnostic: DiagnosticLog::new(command, start),
        }
    }

    /// Return the diagnostic log, if available.
    pub const fn diagnostic(&self) -> Option<&DiagnosticLog> {
        self.diagnostic.as_ref()
    }

    /// Return the log file path, if available.
    #[cfg(test)]
    pub const fn log_path(&self) -> Option<&PathBuf> {
        self.log_file.as_ref()
    }

    /// Log an error message.
    pub fn error(&self, msg: &str) {
        if let Some(d) = &self.diagnostic {
            d.emit(DiagEvent::Error, msg);
        }
        tracing::error!("{msg}");
    }

    /// Log a warning message.
    pub fn warn(&self, msg: &str) {
        if let Some(d) = &self.diagnostic {
            d.emit(DiagEvent::Warn, msg);
        }
        tracing::warn!("{msg}");
    }

    /// Log a stage header (major section).
    pub fn stage(&self, msg: &str) {
        if let Some(d) = &self.diagnostic {
            d.emit(DiagEvent::Stage, msg);
        }
        tracing::info!(target: "dotfiles::stage", "{msg}");
    }

    /// Log an informational message.
    pub fn info(&self, msg: &str) {
        if let Some(d) = &self.diagnostic {
            d.emit(DiagEvent::Info, msg);
        }
        tracing::info!("{msg}");
    }

    /// Log a debug message (suppressed on console unless verbose; always
    /// written to the log file via the [`FileLayer`]).
    pub fn debug(&self, msg: &str) {
        if let Some(d) = &self.diagnostic {
            d.emit(DiagEvent::Debug, msg);
        }
        tracing::debug!("{msg}");
    }

    /// Log a dry-run action message.
    pub fn dry_run(&self, msg: &str) {
        if let Some(d) = &self.diagnostic {
            d.emit(DiagEvent::DryRun, msg);
        }
        tracing::info!(target: "dotfiles::dry_run", "{msg}");
    }

    /// Record a task result for the summary.
    pub fn record_task(&self, name: &str, status: TaskStatus, message: Option<&str>) {
        if let Ok(mut guard) = self.tasks.lock() {
            guard.push(TaskEntry {
                name: name.to_string(),
                status,
                message: message.map(String::from),
            });
        }
    }

    /// Return `true` if any recorded task has failed.
    #[must_use]
    #[allow(dead_code)] // Used in tests and part of public API
    pub fn has_failures(&self) -> bool {
        self.failure_count() > 0
    }

    /// Count the number of failed tasks.
    #[must_use]
    pub fn failure_count(&self) -> usize {
        self.tasks.lock().map_or(0, |guard| {
            guard
                .iter()
                .filter(|t| t.status == TaskStatus::Failed)
                .count()
        })
    }

    /// Print the summary of all recorded tasks.
    pub fn print_summary(&self) {
        let tasks = match self.tasks.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        if tasks.is_empty() {
            return;
        }

        println!();
        self.stage("Summary");

        let mut ok = 0u32;
        let mut not_applicable = 0u32;
        let mut skipped = 0u32;
        let mut dry_run = 0u32;
        let mut failed = 0u32;

        for task in &tasks {
            let (icon, color) = match task.status {
                TaskStatus::Ok => {
                    ok += 1;
                    ("✓", "\x1b[32m")
                }
                TaskStatus::NotApplicable => {
                    not_applicable += 1;
                    ("·", "\x1b[2m")
                }
                TaskStatus::Skipped => {
                    skipped += 1;
                    ("○", "\x1b[33m")
                }
                TaskStatus::DryRun => {
                    dry_run += 1;
                    ("~", "\x1b[37m")
                }
                TaskStatus::Failed => {
                    failed += 1;
                    ("✗", "\x1b[31m")
                }
            };

            let suffix = task
                .message
                .as_ref()
                .map_or_else(String::new, |msg| format!(" ({msg})"));

            // Emit as a tracing event so the FileLayer writes it to the log
            // file (with ANSI stripped) alongside the console output.
            self.info(&format!("{color}{icon} {}{suffix}\x1b[0m", task.name));
        }

        println!();
        let total = ok + not_applicable + skipped + dry_run + failed;
        self.info(&format!(
            "{total} tasks: \x1b[32m{ok} ok\x1b[0m, \x1b[2m{not_applicable} n/a\x1b[0m, \x1b[33m{skipped} skipped\x1b[0m, \x1b[37m{dry_run} dry-run\x1b[0m, \x1b[31m{failed} failed\x1b[0m"
        ));

        if let Some(path) = &self.log_file {
            self.info(&format!("\x1b[2mlog: {}\x1b[0m", path.display()));
        }
    }

    /// Erase the in-progress status line from the console.
    ///
    /// No-op if no progress line is currently shown.
    /// Must be called while holding `flush_lock`.
    fn clear_progress(&self) {
        let mut guard = self
            .progress_rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *guard > 0 {
            print!("\r\x1b[K");
            std::io::stdout().flush().ok();
            *guard = 0;
        }
    }

    /// Print an in-progress status line to the console and mark it as shown.
    ///
    /// The task-name list is truncated to fit within a single terminal row so
    /// that [`clear_progress`](Self::clear_progress) never needs cursor-up
    /// movement (which is fragile when the terminal width is unknown).
    ///
    /// Must be called while holding `flush_lock`.
    fn draw_progress(&self, names: &str) {
        let cols = terminal_columns();
        // Visible prefix is "  ▹ " — 4 display columns.
        let prefix_width = 4;
        let max_name_chars = cols.saturating_sub(prefix_width);
        let display_names = if names.chars().count() > max_name_chars {
            let truncated: String = names
                .chars()
                .take(max_name_chars.saturating_sub(1))
                .collect();
            format!("{truncated}…")
        } else {
            names.to_string()
        };
        print!("  \x1b[2m▹ {display_names}\x1b[0m");
        std::io::stdout().flush().ok();
        let mut guard = self
            .progress_rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = 1;
    }

    /// Record that a parallel task has started.
    ///
    /// Acquires the flush lock, erases any previous progress line, adds the
    /// task to the active set, and redraws the status line.
    pub fn notify_task_start(&self, name: &str) {
        let _guard = self
            .flush_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.clear_progress();
        let names = self.active_tasks.lock().map_or_else(
            |_| name.to_string(),
            |mut active| {
                active.push(name.to_string());
                active.join(", ")
            },
        );
        self.draw_progress(&names);
    }
}

impl Log for Logger {
    forward_log_methods!(stage, info, debug, warn, error, dry_run);

    fn record_task(&self, name: &str, status: TaskStatus, message: Option<&str>) {
        self.record_task(name, status, message);
    }

    fn diagnostic(&self) -> Option<&DiagnosticLog> {
        self.diagnostic.as_ref()
    }
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
struct FileLayer {
    file: Mutex<fs::File>,
}

impl FileLayer {
    /// Open (or create) the log file for `command`, write a run header, and
    /// return a new `FileLayer` ready to receive events.
    ///
    /// Returns `None` if the cache directory cannot be created or the file
    /// cannot be opened.
    fn new(command: &str) -> Option<Self> {
        let path = log_file_path(command)?;
        let version =
            option_env!("DOTFILES_VERSION").unwrap_or(concat!("dev-", env!("CARGO_PKG_VERSION")));
        let header = format!(
            "==========================================\n\
             Dotfiles {version} {}\n\
             ==========================================\n",
            format_utc_datetime(),
        );
        // Truncate to start a fresh log for this run, then re-open for append.
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
            writeln!(f, "{line}").ok(); // Intentionally ignore: logging failure is non-fatal
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

    // Route WARN and ERROR to stderr; INFO and DEBUG to stdout.
    let make_writer = std::io::stderr
        .with_max_level(tracing::Level::WARN)
        .and(std::io::stdout.with_min_level(tracing::Level::INFO));

    let console_layer = fmt::layer()
        .event_format(DotfilesFormatter)
        .with_writer(make_writer)
        .with_filter(console_level);

    // The file layer always captures DEBUG and above, independent of the
    // console verbosity setting.  Option<Layer> is supported by tracing-subscriber
    // and simply becomes a no-op when None.
    let file_layer = FileLayer::new(command).map(|l| l.with_filter(LevelFilter::DEBUG));

    tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .init();
}

#[cfg(test)]
#[allow(unsafe_code)] // set_var/remove_var require unsafe since Rust 1.83
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Serializes `XDG_CACHE_HOME` manipulation across parallel test threads.
    ///
    /// Tests that call `isolated_logger()` must hold this lock for the entire
    /// duration of the env-var set/create/remove sequence to prevent one test
    /// from reading another test's temporary directory.
    static TEST_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Create a Logger backed by an isolated per-thread tracing subscriber
    /// with a [`FileLayer`], so that tracing events emitted by logger methods
    /// actually reach the log file during tests.
    ///
    /// Returns a [`tracing::dispatcher::DefaultGuard`] that must be kept alive
    /// for the duration of the test — dropping it restores the previous
    /// thread-local dispatcher.
    fn isolated_logger() -> (Logger, tempfile::TempDir, tracing::dispatcher::DefaultGuard) {
        use tracing_subscriber::{Layer as _, filter::LevelFilter, layer::SubscriberExt as _};

        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        // Acquire the mutex before touching the env var so that parallel test
        // threads cannot read each other's XDG_CACHE_HOME values.
        let env_lock = TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: Protected by TEST_ENV_MUTEX; the env var is restored to its
        // original state before the lock is released.
        unsafe {
            std::env::set_var("XDG_CACHE_HOME", tmp.path());
        }
        let file_layer = FileLayer::new("test").expect("failed to create file layer");
        let log = Logger::new("test");
        unsafe {
            std::env::remove_var("XDG_CACHE_HOME");
        }
        drop(env_lock);

        let subscriber =
            tracing_subscriber::registry().with(file_layer.with_filter(LevelFilter::DEBUG));
        let guard = tracing::dispatcher::set_default(&tracing::Dispatch::new(subscriber));

        (log, tmp, guard)
    }

    #[test]
    fn logger_new() {
        let (log, _tmp, _guard) = isolated_logger();
        assert!(
            log.tasks.lock().unwrap().is_empty(),
            "expected empty task list"
        );
    }

    #[test]
    fn record_task_ok() {
        let (log, _tmp, _guard) = isolated_logger();
        log.record_task("symlinks", TaskStatus::Ok, None);
        let tasks = log.tasks.lock().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "symlinks");
        assert_eq!(tasks[0].status, TaskStatus::Ok);
        drop(tasks);
    }

    #[test]
    fn record_task_with_message() {
        let (log, _tmp, _guard) = isolated_logger();
        log.record_task("packages", TaskStatus::Skipped, Some("not on arch"));
        assert_eq!(
            log.tasks.lock().unwrap()[0].message,
            Some("not on arch".to_string())
        );
    }

    #[test]
    fn record_multiple_tasks() {
        let (log, _tmp, _guard) = isolated_logger();
        log.record_task("a", TaskStatus::Ok, None);
        log.record_task("b", TaskStatus::Failed, Some("error"));
        log.record_task("c", TaskStatus::DryRun, None);
        assert_eq!(log.tasks.lock().unwrap().len(), 3);
    }

    #[test]
    fn has_failures_detects_failed_task() {
        let (log, _tmp, _guard) = isolated_logger();
        assert!(!log.has_failures());
        log.record_task("a", TaskStatus::Ok, None);
        assert!(!log.has_failures());
        log.record_task("b", TaskStatus::Failed, Some("error"));
        assert!(log.has_failures());
    }

    #[test]
    fn strip_ansi_removes_colors() {
        assert_eq!(strip_ansi("\x1b[31mERROR\x1b[0m hello"), "ERROR hello");
        assert_eq!(strip_ansi("no codes here"), "no codes here");
        assert_eq!(
            strip_ansi("\x1b[1;34m==>\x1b[0m \x1b[1mstage\x1b[0m"),
            "==> stage"
        );
    }

    #[test]
    fn strip_ansi_handles_csi_sequences() {
        // Cursor movement (ends in 'H')
        assert_eq!(strip_ansi("\x1b[2;5Htext"), "text");
        // Erase display (ends in 'J')
        assert_eq!(strip_ansi("\x1b[2Jhello"), "hello");
        // Erase line (ends in 'K')
        assert_eq!(strip_ansi("\x1b[Kworld"), "world");
        // Mixed: SGR + cursor + text
        assert_eq!(strip_ansi("\x1b[31m\x1b[2JERROR\x1b[0m"), "ERROR");
        // Non-CSI two-char escape: ESC + char consumes exactly one extra char
        assert_eq!(strip_ansi("\x1bMtext"), "text");
        // DEC save/restore cursor (ESC 7 / ESC 8) — two-char escapes
        assert_eq!(strip_ansi("\x1b7text"), "text");
        assert_eq!(strip_ansi("\x1b8text"), "text");
    }

    #[test]
    fn log_file_is_created() {
        let (log, _tmp, _guard) = isolated_logger();
        let path = log.log_path().expect("log path should exist");
        assert!(path.exists(), "log file should be created on Logger::new");
    }

    #[test]
    fn debug_always_written_to_file() {
        let (log, _tmp, _guard) = isolated_logger();
        let marker = format!("debug-marker-{}", std::process::id());
        log.debug(&marker);
        let path = log.log_path().expect("log path should exist");
        let contents = fs::read_to_string(path).unwrap();
        assert!(
            contents.contains(&marker),
            "debug messages should always appear in the log file"
        );
    }

    #[test]
    fn days_to_ymd_unix_epoch() {
        // Day 0 = January 1, 1970
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_day_one() {
        // Day 1 = January 2, 1970
        assert_eq!(days_to_ymd(1), (1970, 1, 2));
    }

    #[test]
    fn days_to_ymd_start_of_1971() {
        // Day 365 = January 1, 1971 (1970 is not a leap year)
        assert_eq!(days_to_ymd(365), (1971, 1, 1));
    }

    #[test]
    fn failure_count_returns_correct_count() {
        let (log, _tmp, _guard) = isolated_logger();
        assert_eq!(log.failure_count(), 0);
        log.record_task("a", TaskStatus::Ok, None);
        log.record_task("b", TaskStatus::Failed, Some("error 1"));
        log.record_task("c", TaskStatus::Failed, Some("error 2"));
        log.record_task("d", TaskStatus::Skipped, None);
        assert_eq!(log.failure_count(), 2);
    }

    // -----------------------------------------------------------------------
    // BufferedLog
    // -----------------------------------------------------------------------

    #[test]
    fn buffered_log_record_task_forwards_to_logger() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        buf.record_task("task-a", TaskStatus::Ok, None);
        // record_task is forwarded immediately — visible on the Logger
        assert_eq!(log.tasks.lock().unwrap().len(), 1);
        assert_eq!(log.tasks.lock().unwrap()[0].name, "task-a");
    }

    #[test]
    fn buffered_log_flush_replays_to_file() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        let marker = format!("buf-marker-{}", std::process::id());
        buf.info(&marker);
        // Before flush, the marker should NOT be in the file yet
        let path = log.log_path().expect("log path");
        let before = fs::read_to_string(path).unwrap();
        assert!(
            !before.contains(&marker),
            "buffered output should not be written before flush"
        );
        buf.flush();
        let after = fs::read_to_string(path).unwrap();
        assert!(
            after.contains(&marker),
            "buffered output should appear after flush"
        );
    }

    #[test]
    fn buffered_log_preserves_entry_order() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        buf.stage("stage-1");
        buf.info("info-1");
        buf.debug("debug-1");
        buf.warn("warn-1");
        buf.flush();
        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();
        let stage_pos = contents.find("stage-1").expect("stage-1 in log");
        let info_pos = contents.find("info-1").expect("info-1 in log");
        let debug_pos = contents.find("debug-1").expect("debug-1 in log");
        let warn_pos = contents.find("warn-1").expect("warn-1 in log");
        assert!(stage_pos < info_pos, "stage before info");
        assert!(info_pos < debug_pos, "info before debug");
        assert!(debug_pos < warn_pos, "debug before warn");
    }

    #[test]
    fn log_trait_delegates_to_logger() {
        let (log, _tmp, _guard) = isolated_logger();
        // Use the Log trait methods via the trait object
        let log_ref: &dyn Log = &log;
        log_ref.record_task("via-trait", TaskStatus::Ok, None);
        assert_eq!(log.tasks.lock().unwrap().len(), 1);
    }

    #[test]
    fn progress_rows_zero_initially() {
        let (log, _tmp, _guard) = isolated_logger();
        assert_eq!(*log.progress_rows.lock().unwrap(), 0);
    }

    #[test]
    fn notify_task_start_sets_progress_rows() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        log.notify_task_start("update");
        assert_eq!(
            *log.progress_rows.lock().unwrap(),
            1,
            "progress_rows should be 1 after notify_task_start"
        );
    }

    #[test]
    fn draw_progress_caps_rows_to_one() {
        let (log, _tmp, _guard) = isolated_logger();
        // A name string much wider than any terminal should still yield 1 row.
        let long_names = "a".repeat(500);
        log.draw_progress(&long_names);
        assert_eq!(
            *log.progress_rows.lock().unwrap(),
            1,
            "progress_rows should always be 1 even for very long names"
        );
    }

    #[test]
    fn flush_and_complete_clears_progress_rows() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        log.notify_task_start("update");
        let buf = BufferedLog::new(Arc::clone(&log));
        buf.flush_and_complete("update");
        assert_eq!(
            *log.progress_rows.lock().unwrap(),
            0,
            "progress_rows should be zero after all tasks complete"
        );
    }

    // -----------------------------------------------------------------------
    // DiagnosticLog
    // -----------------------------------------------------------------------

    #[test]
    fn diagnostic_log_is_created() {
        let (log, _tmp, _guard) = isolated_logger();
        let diag = log
            .diagnostic
            .as_ref()
            .expect("diagnostic log should exist");
        assert!(diag.path.exists(), "diagnostic log file should be created");
    }

    #[test]
    fn diagnostic_log_has_header() {
        let (log, _tmp, _guard) = isolated_logger();
        let diag = log.diagnostic.as_ref().expect("diagnostic log");
        let contents = fs::read_to_string(&diag.path).unwrap();
        assert!(
            contents.starts_with("# Diagnostic log"),
            "diagnostic log should start with header"
        );
        assert!(
            contents.contains("elapsed_us"),
            "header should describe columns"
        );
    }

    #[test]
    fn diagnostic_emit_writes_event() {
        let (log, _tmp, _guard) = isolated_logger();
        let diag = log.diagnostic.as_ref().expect("diagnostic log");
        let marker = format!("diag-marker-{}", std::process::id());
        diag.emit(DiagEvent::Info, &marker);
        let contents = fs::read_to_string(&diag.path).unwrap();
        assert!(
            contents.contains(&marker),
            "diagnostic event should appear in diag log"
        );
        assert!(
            contents.contains("INFO"),
            "diagnostic event should have INFO tag"
        );
    }

    #[test]
    fn diagnostic_emit_has_microsecond_precision() {
        let (log, _tmp, _guard) = isolated_logger();
        let diag = log.diagnostic.as_ref().expect("diagnostic log");
        diag.emit(DiagEvent::Stage, "precision-test");
        let contents = fs::read_to_string(&diag.path).unwrap();
        // Look for the wall-clock timestamp with microseconds: T...Z pattern
        let has_us = contents
            .lines()
            .any(|l| l.contains("precision-test") && l.contains('T') && l.contains('Z'));
        assert!(
            has_us,
            "diagnostic should contain microsecond wall-clock timestamp"
        );
    }

    #[test]
    fn diagnostic_emit_task_includes_task_name() {
        let (log, _tmp, _guard) = isolated_logger();
        let diag = log.diagnostic.as_ref().expect("diagnostic log");
        diag.emit_task(DiagEvent::TaskStart, "Install symlinks", "deps satisfied");
        let contents = fs::read_to_string(&diag.path).unwrap();
        assert!(
            contents.contains("[Install symlinks]"),
            "diagnostic task event should include task name in brackets"
        );
        assert!(
            contents.contains("TASK_START"),
            "diagnostic task event should have TASK_START tag"
        );
    }

    #[test]
    fn diagnostic_log_accessible_via_trait() {
        let (log, _tmp, _guard) = isolated_logger();
        let log_ref: &dyn Log = &log;
        assert!(
            log_ref.diagnostic().is_some(),
            "diagnostic() should be accessible via Log trait"
        );
    }

    #[test]
    fn buffered_log_writes_to_diagnostic_immediately() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        let marker = format!("buf-diag-{}", std::process::id());
        buf.info(&marker);
        // Diagnostic should have the entry immediately (before flush)
        let diag = log.diagnostic.as_ref().expect("diagnostic log");
        let contents = fs::read_to_string(&diag.path).unwrap();
        assert!(
            contents.contains(&marker),
            "BufferedLog should write to diagnostic immediately, not after flush"
        );
    }

    #[test]
    fn diagnostic_resource_events_have_correct_tags() {
        let (log, _tmp, _guard) = isolated_logger();
        let diag = log.diagnostic.as_ref().expect("diagnostic log");
        diag.emit(DiagEvent::ResourceCheck, "~/.bashrc state=Missing");
        diag.emit(DiagEvent::ResourceApply, "link ~/.bashrc");
        diag.emit(DiagEvent::ResourceResult, "~/.bashrc applied");
        let contents = fs::read_to_string(&diag.path).unwrap();
        assert!(contents.contains("RES_CHECK"));
        assert!(contents.contains("RES_APPLY"));
        assert!(contents.contains("RES_RESULT"));
    }

    #[test]
    fn diagnostic_events_are_chronologically_ordered() {
        let (log, _tmp, _guard) = isolated_logger();
        let diag = log.diagnostic.as_ref().expect("diagnostic log");
        diag.emit(DiagEvent::Stage, "first");
        std::thread::sleep(std::time::Duration::from_millis(1));
        diag.emit(DiagEvent::Info, "second");
        let contents = fs::read_to_string(&diag.path).unwrap();
        let first_pos = contents.find("first").expect("first in log");
        let second_pos = contents.find("second").expect("second in log");
        assert!(
            first_pos < second_pos,
            "events should appear in chronological order"
        );
    }
}
