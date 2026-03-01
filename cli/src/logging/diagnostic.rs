//! High-precision diagnostic log for capturing the real-time sequence of events.
use std::cell::RefCell;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use super::utils::{diag_log_file_path, format_utc_datetime_us, strip_ansi};

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
#[must_use]
pub fn diag_thread_name() -> String {
    let thread = std::thread::current();
    if let Some(name) = thread.name() {
        return name.to_string();
    }
    DIAG_TASK_NAME.with(|cell| cell.borrow().as_deref().unwrap_or("?").to_string())
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

impl DiagnosticLog {
    /// Create a new diagnostic log file for the given command.
    ///
    /// Returns `None` if the cache directory cannot be created or the file
    /// cannot be opened.
    pub(super) fn new(command: &str, start: Instant) -> Option<Self> {
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

    /// Return the path of the diagnostic log file (test-only).
    #[cfg(test)]
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::fs;

    fn isolated_diag_log() -> (DiagnosticLog, tempfile::TempDir) {
        let _lock = crate::logging::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tmp = tempfile::tempdir().expect("tempdir");
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("XDG_CACHE_HOME", tmp.path());
        }
        let diag = DiagnosticLog::new("test", Instant::now()).expect("diag log");
        #[allow(unsafe_code)]
        unsafe {
            std::env::remove_var("XDG_CACHE_HOME");
        }
        (diag, tmp)
    }

    #[test]
    fn diagnostic_log_is_created() {
        let (diag, _tmp) = isolated_diag_log();
        assert!(
            diag.path().exists(),
            "diagnostic log file should be created"
        );
    }

    #[test]
    fn diagnostic_log_has_header() {
        let (diag, _tmp) = isolated_diag_log();
        let contents = fs::read_to_string(diag.path()).unwrap();
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
        let (diag, _tmp) = isolated_diag_log();
        let marker = format!("diag-marker-{}", std::process::id());
        diag.emit(DiagEvent::Info, &marker);
        let contents = fs::read_to_string(diag.path()).unwrap();
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
        let (diag, _tmp) = isolated_diag_log();
        diag.emit(DiagEvent::Stage, "precision-test");
        let contents = fs::read_to_string(diag.path()).unwrap();
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
        let (diag, _tmp) = isolated_diag_log();
        diag.emit_task(DiagEvent::TaskStart, "Install symlinks", "deps satisfied");
        let contents = fs::read_to_string(diag.path()).unwrap();
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
    fn diagnostic_resource_events_have_correct_tags() {
        let (diag, _tmp) = isolated_diag_log();
        diag.emit(DiagEvent::ResourceCheck, "~/.bashrc state=Missing");
        diag.emit(DiagEvent::ResourceApply, "link ~/.bashrc");
        diag.emit(DiagEvent::ResourceResult, "~/.bashrc applied");
        let contents = fs::read_to_string(diag.path()).unwrap();
        assert!(contents.contains("RES_CHECK"));
        assert!(contents.contains("RES_APPLY"));
        assert!(contents.contains("RES_RESULT"));
    }

    #[test]
    fn diagnostic_events_are_chronologically_ordered() {
        let (diag, _tmp) = isolated_diag_log();
        diag.emit(DiagEvent::Stage, "first");
        std::thread::sleep(std::time::Duration::from_millis(1));
        diag.emit(DiagEvent::Info, "second");
        let contents = fs::read_to_string(diag.path()).unwrap();
        let first_pos = contents.find("first").expect("first in log");
        let second_pos = contents.find("second").expect("second in log");
        assert!(
            first_pos < second_pos,
            "events should appear in chronological order"
        );
    }

    #[test]
    fn diag_event_tag_info() {
        assert_eq!(DiagEvent::Info.tag(), "INFO");
    }

    #[test]
    fn diag_event_tag_debug() {
        assert_eq!(DiagEvent::Debug.tag(), "DEBUG");
    }

    #[test]
    fn diag_event_tag_warn() {
        assert_eq!(DiagEvent::Warn.tag(), "WARN");
    }

    #[test]
    fn diag_event_tag_error() {
        assert_eq!(DiagEvent::Error.tag(), "ERROR");
    }

    #[test]
    fn diag_event_tag_stage() {
        assert_eq!(DiagEvent::Stage.tag(), "STAGE");
    }

    #[test]
    fn diag_event_tag_dry_run() {
        assert_eq!(DiagEvent::DryRun.tag(), "DRYRUN");
    }

    #[test]
    fn diag_event_tag_task_events() {
        assert_eq!(DiagEvent::TaskWait.tag(), "TASK_WAIT");
        assert_eq!(DiagEvent::TaskStart.tag(), "TASK_START");
        assert_eq!(DiagEvent::TaskDone.tag(), "TASK_DONE");
        assert_eq!(DiagEvent::TaskSkip.tag(), "TASK_SKIP");
    }

    #[test]
    fn diag_event_tag_resource_events() {
        assert_eq!(DiagEvent::ResourceCheck.tag(), "RES_CHECK");
        assert_eq!(DiagEvent::ResourceApply.tag(), "RES_APPLY");
        assert_eq!(DiagEvent::ResourceResult.tag(), "RES_RESULT");
        assert_eq!(DiagEvent::ResourceRemove.tag(), "RES_REMOVE");
    }

    #[test]
    fn diag_thread_name_returns_nonempty() {
        let name = diag_thread_name();
        assert!(
            !name.is_empty(),
            "diag_thread_name should never return empty string"
        );
    }

    #[test]
    fn set_diag_thread_name_is_retrieved_on_unnamed_thread() {
        let result = std::thread::spawn(|| {
            set_diag_thread_name("my-task");
            diag_thread_name()
        })
        .join()
        .expect("thread should not panic");
        assert_eq!(result, "my-task");
    }

    #[test]
    fn diagnostic_emit_task_empty_message() {
        let (diag, _tmp) = isolated_diag_log();
        diag.emit_task(DiagEvent::TaskDone, "task-name", "");
        let contents = fs::read_to_string(diag.path()).unwrap();
        assert!(
            contents.contains("[task-name]"),
            "should contain [task-name]"
        );
        // Should not have extra space after the bracket
        assert!(
            !contents.contains("[task-name] \n"),
            "should not have trailing space"
        );
    }

    #[test]
    fn diagnostic_strips_ansi_from_message() {
        let (diag, _tmp) = isolated_diag_log();
        diag.emit(DiagEvent::Info, "\x1b[31mred-message\x1b[0m");
        let contents = fs::read_to_string(diag.path()).unwrap();
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
