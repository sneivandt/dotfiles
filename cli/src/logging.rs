use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

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

/// Buffered logger for parallel task execution.
///
/// Captures display output (stage, info, debug, etc.) in memory so that
/// parallel tasks do not interleave their console output.  The captured
/// entries are replayed in order when [`flush`](BufferedLog::flush) is called.
///
/// [`record_task`](Log::record_task) is forwarded directly to the underlying
/// [`Logger`] because the summary collection is already thread-safe.
#[derive(Debug)]
pub struct BufferedLog<'a> {
    inner: &'a Logger,
    entries: Mutex<Vec<LogEntry>>,
}

impl<'a> BufferedLog<'a> {
    /// Create a new buffered logger backed by the given [`Logger`].
    #[must_use]
    pub const fn new(inner: &'a Logger) -> Self {
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
            match entry {
                LogEntry::Stage(msg) => self.inner.stage(msg),
                LogEntry::Info(msg) => self.inner.info(msg),
                LogEntry::Debug(msg) => self.inner.debug(msg),
                LogEntry::Warn(msg) => self.inner.warn(msg),
                LogEntry::Error(msg) => self.inner.error(msg),
                LogEntry::DryRun(msg) => self.inner.dry_run(msg),
            }
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
            match entry {
                LogEntry::Stage(msg) => self.inner.stage(msg),
                LogEntry::Info(msg) => self.inner.info(msg),
                LogEntry::Debug(msg) => self.inner.debug(msg),
                LogEntry::Warn(msg) => self.inner.warn(msg),
                LogEntry::Error(msg) => self.inner.error(msg),
                LogEntry::DryRun(msg) => self.inner.dry_run(msg),
            }
        }
        let remaining = self.inner.active_tasks.lock().map_or(None, |mut active| {
            active.retain(|n| n != task_name);
            if active.is_empty() {
                None
            } else {
                Some(active.join(", "))
            }
        });
        if let Some(names) = remaining {
            self.inner.draw_progress(&names);
        }
    }
}

impl Log for BufferedLog<'_> {
    fn stage(&self, msg: &str) {
        if let Ok(mut guard) = self.entries.lock() {
            guard.push(LogEntry::Stage(msg.to_string()));
        }
    }

    fn info(&self, msg: &str) {
        if let Ok(mut guard) = self.entries.lock() {
            guard.push(LogEntry::Info(msg.to_string()));
        }
    }

    fn debug(&self, msg: &str) {
        if let Ok(mut guard) = self.entries.lock() {
            guard.push(LogEntry::Debug(msg.to_string()));
        }
    }

    fn warn(&self, msg: &str) {
        if let Ok(mut guard) = self.entries.lock() {
            guard.push(LogEntry::Warn(msg.to_string()));
        }
    }

    fn error(&self, msg: &str) {
        if let Ok(mut guard) = self.entries.lock() {
            guard.push(LogEntry::Error(msg.to_string()));
        }
    }

    fn dry_run(&self, msg: &str) {
        if let Ok(mut guard) = self.entries.lock() {
            guard.push(LogEntry::DryRun(msg.to_string()));
        }
    }

    fn record_task(&self, name: &str, status: TaskStatus, message: Option<&str>) {
        // Forward directly — the Logger's task list is already Mutex-protected.
        self.inner.record_task(name, status, message);
    }
}

/// Structured logger with dry-run awareness and summary collection.
///
/// All messages are always written to a persistent log file at
/// `$XDG_CACHE_HOME/dotfiles/<command>.log` (default `~/.cache/dotfiles/<command>.log`)
/// with timestamps and ANSI codes stripped, regardless of the verbose flag.
#[derive(Debug)]
pub struct Logger {
    verbose: bool,
    tasks: Mutex<Vec<TaskEntry>>,
    log_file: Option<PathBuf>,
    /// Serializes console output from parallel task flushes.
    flush_lock: Mutex<()>,
    /// Names of tasks currently executing in parallel.
    active_tasks: Mutex<Vec<String>>,
    /// Whether a progress line is currently displayed on the console.
    progress_shown: Mutex<bool>,
}

/// Return the log file path under `$XDG_CACHE_HOME/dotfiles/` (or `~/.cache/dotfiles/`).
fn log_file_path(command: &str) -> Option<PathBuf> {
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
    Some(dir.join(format!("{command}.log")))
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

/// Format the current UTC time as `YYYY-MM-DD HH:MM:SS`.
fn format_utc_datetime() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d) = days_to_ymd(secs / 86400);
    let day_secs = secs % 86400;
    let h = day_secs / 3600;
    let mi = (day_secs % 3600) / 60;
    let s = day_secs % 60;
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

/// Format the current UTC time as `HH:MM:SS`.
fn format_utc_time() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let day_secs = secs % 86400;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
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
    /// Create a new logger, writing a fresh header to the log file.
    #[must_use]
    pub fn new(verbose: bool, command: &str) -> Self {
        let log_file = log_file_path(command);

        // Write header to log file
        if let Some(ref path) = log_file {
            let version = option_env!("DOTFILES_VERSION")
                .unwrap_or(concat!("dev-", env!("CARGO_PKG_VERSION")));
            let header = format!(
                "==========================================\n\
                 Dotfiles {version} {}\n\
                 ==========================================\n",
                format_utc_datetime(),
            );
            // Truncate and write header (new run = fresh log)
            fs::write(path, header).ok(); // Intentionally ignore: logging failure is non-fatal
        }

        Self {
            verbose,
            tasks: Mutex::new(Vec::new()),
            log_file,
            flush_lock: Mutex::new(()),
            active_tasks: Mutex::new(Vec::new()),
            progress_shown: Mutex::new(false),
        }
    }

    /// Append a line to the persistent log file.
    ///
    /// The file format mirrors the console hierarchy: stage headers get an `==>`
    /// prefix, info lines are indented, and other levels use bracketed labels
    /// (`[error]`, `[warn]`, `[debug]`, `[dry run]`) for easy scanning.
    fn write_to_file(&self, level: &str, msg: &str) {
        if let Some(ref path) = self.log_file
            && let Ok(mut f) = fs::OpenOptions::new().append(true).open(path)
        {
            let ts = format_utc_time();
            let clean = strip_ansi(msg);
            let line = match level {
                "STG" => format!("[{ts}] ==> {clean}"),
                "ERR" => format!("[{ts}]     [error] {clean}"),
                "WRN" => format!("[{ts}]     [warn] {clean}"),
                "DBG" => format!("[{ts}]     [debug] {clean}"),
                "DRY" => format!("[{ts}]     [dry run] {clean}"),
                _ => format!("[{ts}]     {clean}"),
            };
            writeln!(f, "{line}").ok(); // Intentionally ignore: logging failure is non-fatal
        }
    }

    /// Return the log file path, if available.
    #[cfg(test)]
    pub const fn log_path(&self) -> Option<&PathBuf> {
        self.log_file.as_ref()
    }

    /// Log an error message to stderr and the log file.
    pub fn error(&self, msg: &str) {
        eprintln!("\x1b[31mERROR\x1b[0m {msg}");
        self.write_to_file("ERR", msg);
    }

    /// Log a warning message to stderr and the log file.
    pub fn warn(&self, msg: &str) {
        eprintln!("\x1b[33mWARN\x1b[0m  {msg}");
        self.write_to_file("WRN", msg);
    }

    /// Log a stage header (major section) to stdout and the log file.
    pub fn stage(&self, msg: &str) {
        println!("\x1b[1;34m==>\x1b[0m \x1b[1m{msg}\x1b[0m");
        self.write_to_file("STG", msg);
    }

    /// Log an informational message to stdout and the log file.
    pub fn info(&self, msg: &str) {
        println!("  {msg}");
        self.write_to_file("INF", msg);
    }

    /// Log a debug message to stdout (if verbose) and always to the log file.
    pub fn debug(&self, msg: &str) {
        if self.verbose {
            println!("  \x1b[2m{msg}\x1b[0m");
        }
        // Always log debug to file, even when not verbose on terminal
        self.write_to_file("DBG", msg);
    }

    /// Log a dry-run action message to stdout and the log file.
    pub fn dry_run(&self, msg: &str) {
        println!("  \x1b[33m[DRY RUN]\x1b[0m {msg}");
        self.write_to_file("DRY", msg);
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
                    ("~", "\x1b[33m")
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

            let line = format!("{icon} {}{suffix}", task.name);
            println!("  {color}{line}\x1b[0m");
            self.write_to_file("INF", &line);
        }

        println!();
        let total = ok + not_applicable + skipped + dry_run + failed;
        let totals = format!(
            "{total} tasks: {ok} ok, {not_applicable} n/a, {skipped} skipped, {dry_run} dry-run, {failed} failed"
        );
        println!(
            "  {total} tasks: \x1b[32m{ok} ok\x1b[0m, \x1b[2m{not_applicable} n/a\x1b[0m, \x1b[33m{skipped} skipped\x1b[0m, {dry_run} dry-run, \x1b[31m{failed} failed\x1b[0m"
        );
        self.write_to_file("INF", &totals);

        if let Some(path) = &self.log_file {
            println!("  \x1b[2mlog: {}\x1b[0m", path.display());
            self.write_to_file("INF", &format!("log: {}", path.display()));
        }
    }

    /// Erase the in-progress status line from the console.
    ///
    /// No-op if no progress line is currently shown.
    /// Must be called while holding `flush_lock`.
    fn clear_progress(&self) {
        let mut shown = self
            .progress_shown
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *shown {
            // Restore the saved cursor position (set just before the progress
            // line was printed) and erase from there to end of screen.  This
            // correctly removes the line even when it wraps across multiple
            // terminal rows.
            print!("\x1b8\x1b[J");
            std::io::stdout().flush().ok();
            *shown = false;
        }
    }

    /// Print an in-progress status line to the console and mark it as shown.
    ///
    /// Must be called while holding `flush_lock`.
    fn draw_progress(&self, names: &str) {
        // Save the cursor position immediately before the progress line so that
        // clear_progress can restore it precisely regardless of terminal width
        // or whether the line wraps to multiple rows.
        println!("\x1b7  \x1b[2m▹ {names}\x1b[0m");
        std::io::stdout().flush().ok();
        let mut shown = self
            .progress_shown
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *shown = true;
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
    fn stage(&self, msg: &str) {
        self.stage(msg);
    }

    fn info(&self, msg: &str) {
        self.info(msg);
    }

    fn debug(&self, msg: &str) {
        self.debug(msg);
    }

    fn warn(&self, msg: &str) {
        self.warn(msg);
    }

    fn error(&self, msg: &str) {
        self.error(msg);
    }

    fn dry_run(&self, msg: &str) {
        self.dry_run(msg);
    }

    fn record_task(&self, name: &str, status: TaskStatus, message: Option<&str>) {
        self.record_task(name, status, message);
    }
}

#[cfg(test)]
#[allow(unsafe_code)] // set_var/remove_var require unsafe since Rust 1.83
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Create a Logger that writes to an isolated temp directory, avoiding
    /// parallel-test races on the shared `~/.cache/dotfiles/install.log` file.
    fn isolated_logger(verbose: bool) -> (Logger, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        // SAFETY: Each test gets its own unique temp dir, so concurrent set_var
        // calls use different values and the var is removed immediately after
        // Logger::new() reads it. The window is minimal and each test's Logger
        // writes to its own isolated path regardless of env var races.
        unsafe {
            std::env::set_var("XDG_CACHE_HOME", tmp.path());
        }
        let log = Logger::new(verbose, "test");
        unsafe {
            std::env::remove_var("XDG_CACHE_HOME");
        }
        (log, tmp)
    }

    #[test]
    fn logger_new() {
        let (log, _tmp) = isolated_logger(false);
        assert!(!log.verbose, "expected verbose=false");
        assert!(
            log.tasks.lock().unwrap().is_empty(),
            "expected empty task list"
        );
    }

    #[test]
    fn logger_verbose() {
        let (log, _tmp) = isolated_logger(true);
        assert!(log.verbose, "expected verbose=true");
    }

    #[test]
    fn record_task_ok() {
        let (log, _tmp) = isolated_logger(false);
        log.record_task("symlinks", TaskStatus::Ok, None);
        let tasks = log.tasks.lock().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "symlinks");
        assert_eq!(tasks[0].status, TaskStatus::Ok);
        drop(tasks);
    }

    #[test]
    fn record_task_with_message() {
        let (log, _tmp) = isolated_logger(false);
        log.record_task("packages", TaskStatus::Skipped, Some("not on arch"));
        assert_eq!(
            log.tasks.lock().unwrap()[0].message,
            Some("not on arch".to_string())
        );
    }

    #[test]
    fn record_multiple_tasks() {
        let (log, _tmp) = isolated_logger(false);
        log.record_task("a", TaskStatus::Ok, None);
        log.record_task("b", TaskStatus::Failed, Some("error"));
        log.record_task("c", TaskStatus::DryRun, None);
        assert_eq!(log.tasks.lock().unwrap().len(), 3);
    }

    #[test]
    fn has_failures_detects_failed_task() {
        let (log, _tmp) = isolated_logger(false);
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
    }

    #[test]
    fn log_file_is_created() {
        let (log, _tmp) = isolated_logger(false);
        let path = log.log_path().expect("log path should exist");
        assert!(path.exists(), "log file should be created on Logger::new");
    }

    #[test]
    fn debug_always_written_to_file() {
        let (log, _tmp) = isolated_logger(false); // verbose=false
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
        let (log, _tmp) = isolated_logger(false);
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
        let (log, _tmp) = isolated_logger(false);
        let buf = BufferedLog::new(&log);
        buf.record_task("task-a", TaskStatus::Ok, None);
        // record_task is forwarded immediately — visible on the Logger
        assert_eq!(log.tasks.lock().unwrap().len(), 1);
        assert_eq!(log.tasks.lock().unwrap()[0].name, "task-a");
    }

    #[test]
    fn buffered_log_flush_replays_to_file() {
        let (log, _tmp) = isolated_logger(false);
        let buf = BufferedLog::new(&log);
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
        let (log, _tmp) = isolated_logger(true); // verbose so debug appears
        let buf = BufferedLog::new(&log);
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
        let (log, _tmp) = isolated_logger(false);
        // Use the Log trait methods via the trait object
        let log_ref: &dyn Log = &log;
        log_ref.record_task("via-trait", TaskStatus::Ok, None);
        assert_eq!(log.tasks.lock().unwrap().len(), 1);
    }
}
