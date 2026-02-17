use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

/// Log level for output messages.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Stage,
    Warn,
    Error,
}

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
    let cache_dir = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".cache")
        });
    let dir = cache_dir.join("dotfiles");
    fs::create_dir_all(&dir).ok()?;
    Some(dir.join("install.log"))
}

/// Strip ANSI escape sequences from a string.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until 'm' (end of SGR sequence)
            for inner in chars.by_ref() {
                if inner == 'm' {
                    break;
                }
            }
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
            let _ = fs::write(path, header);
        }

        Self {
            verbose,
            tasks: std::cell::RefCell::new(Vec::new()),
            log_file,
        }
    }

    /// Append a line to the persistent log file.
    fn write_to_file(&self, level: &str, msg: &str) {
        if let Some(ref path) = self.log_file
            && let Ok(mut f) = fs::OpenOptions::new().append(true).open(path)
        {
            let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let clean = strip_ansi(msg);
            let _ = writeln!(f, "{ts} {level} {clean}");
        }
    }

    /// Return the log file path, if available.
    #[cfg(test)]
    pub fn log_path(&self) -> Option<&PathBuf> {
        self.log_file.as_ref()
    }

    pub fn error(&self, msg: &str) {
        eprintln!("\x1b[31mERROR\x1b[0m {msg}");
        self.write_to_file("ERR", msg);
    }

    pub fn warn(&self, msg: &str) {
        eprintln!("\x1b[33mWARN\x1b[0m  {msg}");
        self.write_to_file("WRN", msg);
    }

    pub fn stage(&self, msg: &str) {
        println!("\x1b[1;34m==>\x1b[0m \x1b[1m{msg}\x1b[0m");
        self.write_to_file("STG", msg);
    }

    pub fn info(&self, msg: &str) {
        println!("  {msg}");
        self.write_to_file("INF", msg);
    }

    pub fn debug(&self, msg: &str) {
        if self.verbose {
            println!("  \x1b[2m{msg}\x1b[0m");
        }
        // Always log debug to file, even when not verbose on terminal
        self.write_to_file("DBG", msg);
    }

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

            let suffix = match &task.message {
                Some(msg) => format!(" ({msg})"),
                None => String::new(),
            };

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

    /// Prompt the user to select from a list of options. Returns the selected index.
    #[allow(dead_code)]
    pub fn prompt_select(&self, prompt: &str, options: &[&str]) -> io::Result<usize> {
        println!("\n{prompt}");
        for (i, option) in options.iter().enumerate() {
            println!("  \x1b[1m{}\x1b[0m) {option}", i + 1);
        }
        print!("\nSelect [1-{}]: ", options.len());
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        let choice: usize = input
            .trim()
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid selection"))?;

        if choice == 0 || choice > options.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "selection out of range",
            ));
        }

        Ok(choice - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logger_new() {
        let log = Logger::new(false);
        assert!(!log.verbose);
        assert!(log.tasks.borrow().is_empty());
    }

    #[test]
    fn logger_verbose() {
        let log = Logger::new(true);
        assert!(log.verbose);
    }

    #[test]
    fn record_task_ok() {
        let log = Logger::new(false);
        log.record_task("symlinks", TaskStatus::Ok, None);
        let tasks = log.tasks.borrow();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "symlinks");
        assert_eq!(tasks[0].status, TaskStatus::Ok);
    }

    #[test]
    fn record_task_with_message() {
        let log = Logger::new(false);
        log.record_task("packages", TaskStatus::Skipped, Some("not on arch"));
        let tasks = log.tasks.borrow();
        assert_eq!(tasks[0].message, Some("not on arch".to_string()));
    }

    #[test]
    fn record_multiple_tasks() {
        let log = Logger::new(false);
        log.record_task("a", TaskStatus::Ok, None);
        log.record_task("b", TaskStatus::Failed, Some("error"));
        log.record_task("c", TaskStatus::DryRun, None);
        assert_eq!(log.tasks.borrow().len(), 3);
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
    fn log_file_is_created() {
        let log = Logger::new(false);
        if let Some(path) = log.log_path() {
            assert!(path.exists(), "log file should be created on Logger::new");
        }
    }

    #[test]
    fn debug_always_written_to_file() {
        let log = Logger::new(false); // verbose=false
        // Write a unique marker so we can find it even with parallel tests
        let marker = format!("debug-marker-{}", std::process::id());
        log.debug(&marker);
        if let Some(path) = log.log_path() {
            let contents = fs::read_to_string(path).unwrap();
            assert!(
                contents.contains(&marker),
                "debug messages should always appear in the log file"
            );
        }
    }
}
