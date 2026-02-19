use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// Task execution result for summary reporting.
#[derive(Debug, Clone)]
pub struct TaskEntry {
    pub name: String,
    pub status: TaskStatus,
    pub message: Option<String>,
}

/// Status of a completed task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Ok,
    NotApplicable,
    Skipped,
    DryRun,
    Failed,
}

/// Structured logger with dry-run awareness and summary collection.
///
/// All messages are always written to a persistent log file at
/// `$XDG_CACHE_HOME/dotfiles/install.log` (default `~/.cache/dotfiles/install.log`)
/// with timestamps and ANSI codes stripped, regardless of the verbose flag.
pub struct Logger {
    verbose: bool,
    tasks: std::cell::RefCell<Vec<TaskEntry>>,
    log_file: Option<PathBuf>,
}

/// Return the log file path under `$XDG_CACHE_HOME/dotfiles/` (or `~/.cache/dotfiles/`).
fn log_file_path() -> Option<PathBuf> {
    let cache_dir = std::env::var("XDG_CACHE_HOME").map_or_else(
        |_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".cache")
        },
        PathBuf::from,
    );
    let dir = cache_dir.join("dotfiles");
    fs::create_dir_all(&dir).ok()?;
    Some(dir.join("install.log"))
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
    #[must_use]
    pub fn new(verbose: bool) -> Self {
        let log_file = log_file_path();

        // Write header to log file
        if let Some(ref path) = log_file {
            let version = option_env!("DOTFILES_VERSION")
                .unwrap_or(concat!("dev-", env!("CARGO_PKG_VERSION")));
            let header = format!(
                "==========================================\n\
                 Dotfiles {version} {}\n\
                 ==========================================\n",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            );
            // Truncate and write header (new run = fresh log)
            fs::write(path, header).ok(); // Intentionally ignore: logging failure is non-fatal
        }

        Self {
            verbose,
            tasks: std::cell::RefCell::new(Vec::new()),
            log_file,
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
            let ts = chrono::Local::now().format("%H:%M:%S");
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
        self.tasks.borrow_mut().push(TaskEntry {
            name: name.to_string(),
            status,
            message: message.map(String::from),
        });
    }

    /// Return `true` if any recorded task has failed.
    #[must_use]
    pub fn has_failures(&self) -> bool {
        self.tasks
            .borrow()
            .iter()
            .any(|t| t.status == TaskStatus::Failed)
    }

    /// Count the number of failed tasks.
    #[must_use]
    pub fn failure_count(&self) -> usize {
        self.tasks
            .borrow()
            .iter()
            .filter(|t| t.status == TaskStatus::Failed)
            .count()
    }

    /// Print the summary of all recorded tasks.
    pub fn print_summary(&self) {
        let tasks = self.tasks.borrow();
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

        for task in tasks.iter() {
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
            "  {total} tasks: \x1b[32m{ok} ok\x1b[0m, {not_applicable} n/a, \x1b[33m{skipped} skipped\x1b[0m, {dry_run} dry-run, \x1b[31m{failed} failed\x1b[0m"
        );
        self.write_to_file("INF", &totals);

        if let Some(path) = &self.log_file {
            println!("  \x1b[2mlog: {}\x1b[0m", path.display());
            self.write_to_file("INF", &format!("log: {}", path.display()));
        }
    }
}

#[cfg(test)]
#[allow(unsafe_code)] // set_var/remove_var require unsafe since Rust 1.83
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
        let log = Logger::new(verbose);
        unsafe {
            std::env::remove_var("XDG_CACHE_HOME");
        }
        (log, tmp)
    }

    #[test]
    fn logger_new() {
        let (log, _tmp) = isolated_logger(false);
        assert!(!log.verbose, "expected verbose=false");
        assert!(log.tasks.borrow().is_empty(), "expected empty task list");
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
        let tasks = log.tasks.borrow();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "symlinks");
        assert_eq!(tasks[0].status, TaskStatus::Ok);
    }

    #[test]
    fn record_task_with_message() {
        let (log, _tmp) = isolated_logger(false);
        log.record_task("packages", TaskStatus::Skipped, Some("not on arch"));
        let tasks = log.tasks.borrow();
        assert_eq!(tasks[0].message, Some("not on arch".to_string()));
    }

    #[test]
    fn record_multiple_tasks() {
        let (log, _tmp) = isolated_logger(false);
        log.record_task("a", TaskStatus::Ok, None);
        log.record_task("b", TaskStatus::Failed, Some("error"));
        log.record_task("c", TaskStatus::DryRun, None);
        assert_eq!(log.tasks.borrow().len(), 3);
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
}
