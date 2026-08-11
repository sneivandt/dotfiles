//! The run log: a single append-only record of everything that happened.
//!
//! Every message is written here immediately, in true chronological order,
//! regardless of whether it is also shown on the console.  Console output is a
//! separate concern handled by the tracing console layer, so this module never
//! needs to know about verbosity, styling, or terminal state.
use std::cell::RefCell;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use super::types::{ExecutionEvent, LogEvent};
use super::utils::{format_utc_compact, format_utc_datetime_us, strip_ansi};

/// Number of run logs retained in the log directory.
///
/// Old runs are pruned oldest-first at the start of each run, so a failure is
/// still readable long after the run that produced it.
const MAX_RETAINED_RUNS: usize = 50;

/// File extension used for run logs.
const LOG_EXTENSION: &str = "log";

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

/// Components of a run-log file name.
///
/// Names are `<stamp>-<command>-<pid>.log`, e.g.
/// `20260731T154210Z-install-48213.log`. The stamp is fixed width so lexical
/// ordering matches chronological ordering, and the pid disambiguates runs
/// that start within the same second — notably the elevated child process
/// spawned on Windows, which is a second process for one logical run.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RunLogName<'a> {
    /// Compact UTC start stamp, `YYYYMMDDTHHMMSSZ`.
    pub(crate) stamp: &'a str,
    /// Command that produced the run.
    pub(crate) command: &'a str,
}

/// Build the run-log file name for a command.
fn run_log_file_name(command: &str, stamp: &str, pid: u32) -> String {
    format!("{stamp}-{command}-{pid}.{LOG_EXTENSION}")
}

/// Parse a run-log file name, or return `None` if it is not one.
///
/// Command names never contain `-`, so splitting into exactly three parts is
/// unambiguous. Files that do not match are ignored rather than pruned or
/// listed, so unrelated files in the log directory are left alone.
pub(crate) fn parse_run_log_file_name(name: &str) -> Option<RunLogName<'_>> {
    let stem = name.strip_suffix(".log")?;
    let mut parts = stem.split('-');
    let stamp = parts.next()?;
    let command = parts.next()?;
    let pid = parts.next()?;
    if parts.next().is_some() || stamp.is_empty() || command.is_empty() {
        return None;
    }
    pid.parse::<u32>().ok()?;
    Some(RunLogName { stamp, command })
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
    #[cfg(test)]
    path: PathBuf,
    start: Instant,
    sequence: AtomicU64,
    healthy: AtomicBool,
}

impl RunLog {
    /// Create the run log for `command` in the resolved dotfiles log
    /// directory.
    ///
    /// Returns `None` if the log directory or file cannot be created, in
    /// which case the run simply proceeds without a file log.
    pub(super) fn create(command: &str, start: Instant) -> Option<Self> {
        super::utils::remove_legacy_cache_logs_once();
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
        let (path, mut file) = create_run_log_file(command, log_dir)?;
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
        #[cfg(not(test))]
        drop(path);
        Some(Self {
            file: Mutex::new(file),
            #[cfg(test)]
            path,
            start,
            sequence: AtomicU64::new(0),
            healthy: AtomicBool::new(true),
        })
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
        self.emit_event(&ExecutionEvent::message(event, message.into()));
    }

    /// Emit an event with an explicit context name.
    fn emit_with_context(&self, event: LogEvent, context: &str, message: &str) {
        self.emit_event(&ExecutionEvent::with_context(
            event,
            context.into(),
            message.into(),
        ));
    }

    /// Deliver one typed execution event to the persistent sink.
    pub(in crate::infra::logging) fn emit_event(&self, event: &ExecutionEvent<'_>) {
        let Some(formatted_message) = format_log_message(&event.message) else {
            return;
        };
        let context = event
            .context
            .as_deref()
            .map_or_else(log_thread_name, str::to_string);
        self.write_event(event.kind, &context, &formatted_message);
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

    /// Emit an event with an explicit task name context.
    pub fn emit_task(&self, event: LogEvent, task: &str, message: &str) {
        self.emit_with_context(event, task, message);
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
    #[cfg(test)]
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Attempts made to find an unused run-log file name before giving up.
const MAX_NAME_ATTEMPTS: u32 = 64;

/// Create a fresh run-log file, returning its path and an append handle.
///
/// Always uses `create_new`, so an existing log is never truncated. When the
/// name is already taken — two runs starting in the same second from the same
/// process — the disambiguating suffix is bumped until a free name is found
/// rather than silently reusing or destroying the existing file.
fn create_run_log_file(command: &str, log_dir: &Path) -> Option<(PathBuf, fs::File)> {
    let stamp = format_utc_compact();
    let pid = std::process::id();
    for attempt in 0..MAX_NAME_ATTEMPTS {
        let path = log_dir.join(run_log_file_name(
            command,
            &stamp,
            pid.wrapping_add(attempt),
        ));
        match fs::OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)
        {
            Ok(file) => return Some((path, file)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return None,
        }
    }
    None
}

/// Delete the oldest run logs until at most `keep` remain.
///
/// Only files matching the run-log naming pattern are considered, so
/// unrelated files in the log directory are never touched. Every failure is
/// ignored, including `NotFound` races with a concurrent run pruning the same
/// directory: retention is housekeeping and must never fail a run.
fn prune_run_logs(dir: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            parse_run_log_file_name(&name).is_some().then_some(name)
        })
        .collect();
    let excess = names.len().saturating_sub(keep);
    if excess == 0 {
        return;
    }
    // Names start with a fixed-width UTC stamp, so ascending lexical order is
    // oldest first.
    names.sort_unstable();
    for name in names.into_iter().take(excess) {
        drop(fs::remove_file(dir.join(name)));
    }
}

fn format_log_message(message: &str) -> Option<String> {
    let clean = strip_ansi(message);
    let mut formatted = String::new();
    for line in clean.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if !formatted.is_empty() {
            formatted.push_str(" | ");
        }
        formatted.push_str(line);
    }
    if formatted.is_empty() {
        None
    } else {
        Some(formatted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn isolated_run_log() -> (RunLog, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let run_log = RunLog::new("test", tmp.path(), Instant::now()).expect("run log");
        (run_log, tmp)
    }

    fn run_log_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("read log dir")
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn run_log_file_name_round_trips() {
        let name = run_log_file_name("install", "20260731T154210Z", 4321);
        assert_eq!(name, "20260731T154210Z-install-4321.log");
        let parsed = parse_run_log_file_name(&name).expect("name should parse");
        assert_eq!(parsed.stamp, "20260731T154210Z");
        assert_eq!(parsed.command, "install");
    }

    #[test]
    fn parse_run_log_file_name_rejects_foreign_names() {
        for name in [
            "install.log",
            "notes.txt",
            "20260731T154210Z-install.log",
            "20260731T154210Z-install-notapid.log",
            "20260731T154210Z-install-1-extra.log",
            "-install-1.log",
        ] {
            assert!(
                parse_run_log_file_name(name).is_none(),
                "{name} should not parse as a run log"
            );
        }
    }

    #[test]
    fn each_run_writes_its_own_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let first = RunLog::new("test", tmp.path(), Instant::now()).expect("first run log");
        let second = RunLog::new("test", tmp.path(), Instant::now()).expect("second run log");

        assert_ne!(
            first.path(),
            second.path(),
            "a second run must not reuse the first run's file"
        );
        assert_eq!(
            run_log_names(tmp.path()).len(),
            2,
            "both runs should be retained"
        );
    }

    #[test]
    fn prune_keeps_the_newest_runs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        for stamp in ["20260731T154210Z", "20260731T154211Z", "20260731T154212Z"] {
            fs::write(tmp.path().join(run_log_file_name("install", stamp, 1)), "x")
                .expect("write log");
        }

        prune_run_logs(tmp.path(), 2);

        assert_eq!(
            run_log_names(tmp.path()),
            vec![
                "20260731T154211Z-install-1.log".to_string(),
                "20260731T154212Z-install-1.log".to_string(),
            ],
            "the oldest run should be pruned first"
        );
    }

    #[test]
    fn prune_ignores_files_that_are_not_run_logs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join("notes.txt"), "keep me").expect("write note");
        fs::write(tmp.path().join("install.log"), "legacy").expect("write legacy log");
        fs::write(
            tmp.path()
                .join(run_log_file_name("install", "20260731T154210Z", 1)),
            "x",
        )
        .expect("write log");

        prune_run_logs(tmp.path(), 0);

        assert_eq!(
            run_log_names(tmp.path()),
            vec!["install.log".to_string(), "notes.txt".to_string()],
            "only run logs should be pruned"
        );
    }

    #[test]
    fn new_prunes_older_runs_beyond_retention() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path()
                .join(run_log_file_name("install", "20200101T000000Z", 1)),
            "old",
        )
        .expect("write old log");

        let run_log =
            RunLog::new_retaining("test", tmp.path(), Instant::now(), 1).expect("run log");

        assert_eq!(
            run_log_names(tmp.path()),
            vec![
                run_log
                    .path()
                    .file_name()
                    .expect("file name")
                    .to_string_lossy()
                    .into_owned()
            ],
            "only the current run should survive a retention of one"
        );
    }

    #[test]
    fn run_log_is_created() {
        let (run_log, _tmp) = isolated_run_log();
        assert!(run_log.path().exists(), "run log file should be created");
    }

    #[test]
    fn run_log_has_header() {
        let (run_log, _tmp) = isolated_run_log();
        let contents = fs::read_to_string(run_log.path()).unwrap();
        assert!(
            contents.starts_with("# Dotfiles "),
            "run log should start with header"
        );
        assert!(
            contents.contains("seq | elapsed_us | wall_utc | context | event | message"),
            "header should describe columns"
        );
    }

    #[test]
    fn run_log_emit_writes_event() {
        let (run_log, _tmp) = isolated_run_log();
        let marker = format!("run-log-marker-{}", std::process::id());
        run_log.emit(LogEvent::Info, &marker);
        let contents = fs::read_to_string(run_log.path()).unwrap();
        assert!(contents.contains(&marker), "event should appear in run log");
        assert!(
            contents.contains("[info]"),
            "run-log event should include info event name"
        );
    }

    #[test]
    fn run_log_event_appears_after_context_without_padding() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit(LogEvent::Warn, "event-order");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        let line = contents
            .lines()
            .find(|l| l.contains("event-order"))
            .unwrap();
        assert!(
            line.contains("] [warn] event-order"),
            "event should be bracketed immediately before the message: {line}"
        );
        assert!(
            !line.contains("[warn]  event-order"),
            "event column should not add padding after the closing bracket: {line}"
        );
    }

    #[test]
    fn run_log_emit_has_microsecond_precision() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit(LogEvent::Stage, "precision-test");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        let has_us = contents
            .lines()
            .any(|l| l.contains("precision-test") && l.contains('T') && l.contains('Z'));
        assert!(
            has_us,
            "run-log should contain microsecond wall-clock timestamp"
        );
    }

    #[test]
    fn run_log_emit_task_includes_task_name() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit_task(LogEvent::TaskStart, "Install symlinks", "deps satisfied");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        assert!(
            contents.contains("[Install symlinks]"),
            "run-log task event should include task name in brackets"
        );
        assert!(
            contents.contains("deps satisfied"),
            "run-log task event should include the message"
        );
    }

    #[test]
    fn typed_event_preserves_explicit_context() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit_event(&ExecutionEvent::with_context(
            LogEvent::TaskDone,
            "typed-task".into(),
            "complete".into(),
        ));
        let contents = fs::read_to_string(run_log.path()).unwrap();
        assert!(contents.contains("[typed-task] [task_done] complete"));
    }

    #[test]
    fn poisoned_file_lock_marks_run_log_unhealthy() {
        let (run_log, _tmp) = isolated_run_log();
        let run_log = std::sync::Arc::new(run_log);
        let poisoner = std::sync::Arc::clone(&run_log);
        drop(
            std::thread::spawn(move || {
                let _guard = poisoner.file.lock().unwrap();
                panic!("poison run-log lock");
            })
            .join(),
        );

        run_log.emit(LogEvent::Info, "after poison");

        assert!(!run_log.is_healthy());
    }

    #[test]
    fn run_log_resource_events_include_messages() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit(LogEvent::ResourceCheck, "~/.bashrc state=Missing");
        run_log.emit(LogEvent::ResourceApply, "link ~/.bashrc");
        run_log.emit(LogEvent::ResourceResult, "~/.bashrc applied");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        assert!(contents.contains("~/.bashrc state=Missing"));
        assert!(contents.contains("link ~/.bashrc"));
        assert!(contents.contains("~/.bashrc applied"));
    }

    #[test]
    fn run_log_events_are_chronologically_ordered() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit(LogEvent::Stage, "first");
        std::thread::sleep(std::time::Duration::from_millis(1));
        run_log.emit(LogEvent::Info, "second");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        let first_pos = contents.find("first").expect("first in log");
        let second_pos = contents.find("second").expect("second in log");
        assert!(
            first_pos < second_pos,
            "events should appear in chronological order"
        );
    }

    #[test]
    fn log_event_names_are_stable_snake_case() {
        assert_eq!(LogEvent::Debug.name(), "debug");
        assert_eq!(LogEvent::Info.name(), "info");
        assert_eq!(LogEvent::Stage.name(), "stage");
        assert_eq!(LogEvent::TaskStart.name(), "task_start");
        assert_eq!(LogEvent::TaskDone.name(), "task_done");
        assert_eq!(LogEvent::ResourceApply.name(), "resource_apply");
        assert_eq!(LogEvent::Warn.name(), "warn");
        assert_eq!(LogEvent::Error.name(), "error");
        assert_eq!(LogEvent::TaskFail.name(), "task_fail");
    }

    #[test]
    fn log_thread_name_returns_nonempty() {
        let name = log_thread_name();
        assert!(
            !name.is_empty(),
            "log_thread_name should never return empty string"
        );
    }

    #[test]
    fn set_log_thread_name_is_retrieved_on_unnamed_thread() {
        let result = std::thread::spawn(|| {
            set_log_thread_name("my-task");
            log_thread_name()
        })
        .join()
        .expect("thread should not panic");
        assert_eq!(result, "my-task");
    }

    #[test]
    fn log_task_context_restores_previous_name() {
        let result = std::thread::spawn(|| {
            set_log_thread_name("outer-task");
            let inner = {
                let _guard = log_task_context("inner-task");
                log_thread_name()
            };
            (inner, log_thread_name())
        })
        .join()
        .expect("thread should not panic");
        assert_eq!(result, ("inner-task".to_string(), "outer-task".to_string()));
    }

    #[test]
    fn run_log_omits_blank_messages() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit_task(LogEvent::TaskDone, "task-name", "");
        run_log.emit(LogEvent::Debug, "   \t");
        run_log.emit(LogEvent::Info, "after blanks");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        assert!(
            !contents.contains("[task-name]"),
            "empty messages should be omitted"
        );
        assert!(
            contents
                .lines()
                .any(|line| line.starts_with("000001 ") && line.contains("after blanks")),
            "blank messages should not consume sequence numbers"
        );
    }

    #[test]
    fn run_log_collapses_multiline_message_without_blank_lines() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit(LogEvent::Info, "first\n\n  second  ");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        let line = contents
            .lines()
            .find(|line| line.contains("first"))
            .unwrap();
        assert!(
            line.ends_with("first | second"),
            "multiline messages should be collapsed: {line}"
        );
        assert!(
            !contents.lines().any(str::is_empty),
            "run-log log should not contain blank lines"
        );
    }

    #[test]
    fn run_log_events_have_sequence_numbers() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit(LogEvent::Info, "first");
        run_log.emit(LogEvent::Info, "second");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        assert!(
            contents.lines().any(|line| line.starts_with("000001 ")),
            "first event should have sequence 1"
        );
        assert!(
            contents.lines().any(|line| line.starts_with("000002 ")),
            "second event should have sequence 2"
        );
    }

    #[test]
    fn run_log_strips_ansi_from_message() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit(LogEvent::Info, "\x1b[31mred-message\x1b[0m");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        assert!(
            contents.contains("red-message"),
            "stripped text should appear"
        );
        assert!(
            !contents.contains("\x1b[31m"),
            "ANSI codes should be stripped"
        );
    }
}
