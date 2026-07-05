//! High-precision diagnostic log for capturing the real-time sequence of events.
use std::cell::RefCell;
use std::fs;
use std::io::Write;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use super::utils::{format_utc_datetime_us, strip_ansi};

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

/// Guard that restores the previous diagnostic task context when dropped.
#[derive(Debug)]
pub struct DiagTaskContextGuard {
    previous: Option<String>,
}

impl Drop for DiagTaskContextGuard {
    fn drop(&mut self) {
        DIAG_TASK_NAME.with(|cell| {
            *cell.borrow_mut() = self.previous.take();
        });
    }
}

/// Set the diagnostic task context for the current scope.
#[must_use]
pub fn diag_task_context(name: &str) -> DiagTaskContextGuard {
    let previous = DIAG_TASK_NAME.with(|cell| cell.borrow_mut().replace(name.to_string()));
    DiagTaskContextGuard { previous }
}

/// Read the diagnostic task name for the current thread.
#[must_use]
pub fn diag_thread_name() -> String {
    if let Some(name) = DIAG_TASK_NAME.with(|cell| cell.borrow().clone()) {
        return name;
    }
    let thread = std::thread::current();
    if let Some(name) = thread.name() {
        return name.to_string();
    }
    "?".to_string()
}

/// Event kinds for the diagnostic log.
///
/// Each variant maps to a stable `snake_case` event name in the log output.
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
    /// A task failed (e.g. returned an error or panicked).
    TaskFail,
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
    /// Stable event name for the log line.
    const fn name(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Stage => "stage",
            Self::DryRun => "dry_run",
            Self::TaskWait => "task_wait",
            Self::TaskStart => "task_start",
            Self::TaskDone => "task_done",
            Self::TaskSkip => "task_skip",
            Self::TaskFail => "task_fail",
            Self::ResourceCheck => "resource_check",
            Self::ResourceApply => "resource_apply",
            Self::ResourceResult => "resource_result",
            Self::ResourceRemove => "resource_remove",
        }
    }
}

/// High-precision diagnostic log for capturing the real-time sequence of events.
///
/// Unlike the main log file (which replays buffered output per-task and uses
/// second-precision timestamps), the diagnostic log writes every event
/// **immediately** with microsecond-precision elapsed time from program start,
/// the originating task/thread context, and the event kind. This makes it possible
/// to reconstruct the true interleaved timeline of parallel execution.
///
/// Written to `$XDG_CACHE_HOME/dotfiles/<command>.diag.log`.
#[derive(Debug)]
pub struct DiagnosticLog {
    file: Mutex<fs::File>,
    #[cfg(test)]
    path: PathBuf,
    start: Instant,
    sequence: AtomicU64,
}

impl DiagnosticLog {
    /// Create a new diagnostic log file for the given command.
    ///
    /// `cache_dir` is the resolved `dotfiles` cache directory (e.g.
    /// `$XDG_CACHE_HOME/dotfiles/`).  The caller is responsible for
    /// resolving the directory; this constructor never reads environment
    /// variables.
    ///
    /// Returns `None` if the file cannot be created.
    pub(super) fn new(command: &str, cache_dir: &Path, start: Instant) -> Option<Self> {
        let path = cache_dir.join(format!("{command}.diag.log"));
        let version =
            option_env!("DOTFILES_VERSION").unwrap_or(concat!("dev-", env!("CARGO_PKG_VERSION")));
        let header = format!(
            "# Diagnostic log — dotfiles {version} {}\n\
             # Columns: seq | elapsed_us | wall_utc | context | event | message\n",
            format_utc_datetime_us(),
        );
        fs::write(&path, header).ok()?;
        let file = fs::OpenOptions::new().append(true).open(&path).ok()?;
        Some(Self {
            file: Mutex::new(file),
            #[cfg(test)]
            path,
            start,
            sequence: AtomicU64::new(0),
        })
    }

    /// Emit a diagnostic event.
    ///
    /// Each line is:
    /// `<seq> +<elapsed_us> <wall_utc_us> [<context>] [<event>] <message>`
    ///
    /// ANSI escape sequences are stripped from the message. The context comes
    /// from the current task context when one is set, otherwise from the OS
    /// thread name when available (e.g. `"main"`). Blank messages are omitted.
    pub fn emit(&self, event: DiagEvent, message: &str) {
        self.emit_with_context(event, &diag_thread_name(), message);
    }

    /// Emit a diagnostic event with an explicit context name.
    fn emit_with_context(&self, event: DiagEvent, context: &str, message: &str) {
        let clean = strip_ansi(message);
        let formatted_message = clean
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" | ");
        if formatted_message.is_empty() {
            return;
        }
        let Ok(mut f) = self.file.lock() else {
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
        drop(f.write_all(line.as_bytes()));
    }

    /// Emit a diagnostic event with an explicit task name context.
    pub fn emit_task(&self, event: DiagEvent, task: &str, message: &str) {
        self.emit_with_context(event, task, message);
    }

    /// Return the path of the diagnostic log file.
    #[cfg(test)]
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use std::fs;

    fn isolated_diag_log() -> (DiagnosticLog, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let diag = DiagnosticLog::new("test", tmp.path(), Instant::now()).expect("diag log");
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
            contents.contains("seq | elapsed_us | wall_utc | context | event | message"),
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
            contents.contains("[info]"),
            "diagnostic event should include info event name"
        );
    }

    #[test]
    fn diagnostic_event_appears_after_context_without_padding() {
        let (diag, _tmp) = isolated_diag_log();
        diag.emit(DiagEvent::Warn, "event-order");
        let contents = fs::read_to_string(diag.path()).unwrap();
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
            contents.contains("deps satisfied"),
            "diagnostic task event should include the message"
        );
    }

    #[test]
    fn diagnostic_resource_events_include_messages() {
        let (diag, _tmp) = isolated_diag_log();
        diag.emit(DiagEvent::ResourceCheck, "~/.bashrc state=Missing");
        diag.emit(DiagEvent::ResourceApply, "link ~/.bashrc");
        diag.emit(DiagEvent::ResourceResult, "~/.bashrc applied");
        let contents = fs::read_to_string(diag.path()).unwrap();
        assert!(contents.contains("~/.bashrc state=Missing"));
        assert!(contents.contains("link ~/.bashrc"));
        assert!(contents.contains("~/.bashrc applied"));
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
    fn diag_event_names_are_stable_snake_case() {
        assert_eq!(DiagEvent::Debug.name(), "debug");
        assert_eq!(DiagEvent::Info.name(), "info");
        assert_eq!(DiagEvent::Stage.name(), "stage");
        assert_eq!(DiagEvent::TaskStart.name(), "task_start");
        assert_eq!(DiagEvent::TaskDone.name(), "task_done");
        assert_eq!(DiagEvent::ResourceApply.name(), "resource_apply");
        assert_eq!(DiagEvent::Warn.name(), "warn");
        assert_eq!(DiagEvent::Error.name(), "error");
        assert_eq!(DiagEvent::TaskFail.name(), "task_fail");
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
    fn diag_task_context_restores_previous_name() {
        let result = std::thread::spawn(|| {
            set_diag_thread_name("outer-task");
            let inner = {
                let _guard = diag_task_context("inner-task");
                diag_thread_name()
            };
            (inner, diag_thread_name())
        })
        .join()
        .expect("thread should not panic");
        assert_eq!(result, ("inner-task".to_string(), "outer-task".to_string()));
    }

    #[test]
    fn diagnostic_omits_blank_messages() {
        let (diag, _tmp) = isolated_diag_log();
        diag.emit_task(DiagEvent::TaskDone, "task-name", "");
        diag.emit(DiagEvent::Debug, "   \t");
        diag.emit(DiagEvent::Info, "after blanks");
        let contents = fs::read_to_string(diag.path()).unwrap();
        assert!(
            !contents.contains("[task-name]"),
            "empty diagnostic messages should be omitted"
        );
        assert!(
            contents
                .lines()
                .any(|line| line.starts_with("000001 ") && line.contains("after blanks")),
            "blank diagnostic messages should not consume sequence numbers"
        );
    }

    #[test]
    fn diagnostic_collapses_multiline_message_without_blank_lines() {
        let (diag, _tmp) = isolated_diag_log();
        diag.emit(DiagEvent::Info, "first\n\n  second  ");
        let contents = fs::read_to_string(diag.path()).unwrap();
        let line = contents
            .lines()
            .find(|line| line.contains("first"))
            .unwrap();
        assert!(
            line.ends_with("first | second"),
            "multiline diagnostic messages should be collapsed: {line}"
        );
        assert!(
            !contents.lines().any(str::is_empty),
            "diagnostic log should not contain blank lines"
        );
    }

    #[test]
    fn diagnostic_events_have_sequence_numbers() {
        let (diag, _tmp) = isolated_diag_log();
        diag.emit(DiagEvent::Info, "first");
        diag.emit(DiagEvent::Info, "second");
        let contents = fs::read_to_string(diag.path()).unwrap();
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
