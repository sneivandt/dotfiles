//! End-of-run summary printing for [`Logger`].
//!
//! Renders final aggregate counts and compact completed-task rows.

use std::time::Duration;

use super::{Logger, TaskDetailEntry};
use crate::logging::types::{TaskEntry, TaskStatus};

impl Logger {
    /// Print the summary of all recorded tasks.
    pub fn print_summary(&self) {
        let tasks = match self.tasks.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        if tasks.is_empty() {
            self.clear_status();
            return;
        }

        self.clear_status();

        let mut changed = 0u32;
        let mut unchanged = 0u32;
        let mut skipped = 0u32;
        let mut dry_run = 0u32;
        let mut failed = 0u32;

        for task in &tasks {
            match task.status {
                TaskStatus::Changed => changed = changed.saturating_add(1),
                TaskStatus::Ok | TaskStatus::NotApplicable => {
                    unchanged = unchanged.saturating_add(1);
                }
                TaskStatus::Skipped => skipped = skipped.saturating_add(1),
                TaskStatus::DryRun => dry_run = dry_run.saturating_add(1),
                TaskStatus::Failed => failed = failed.saturating_add(1),
            }
        }

        let elapsed = self.start.elapsed();
        let elapsed_str = format_elapsed(elapsed);

        let summary_mode = SummaryMode::for_command(&self.command);
        let status_line = format_summary_counts(
            changed,
            unchanged,
            skipped,
            dry_run,
            failed,
            summary_mode,
            self.dry_run,
        );
        let (text_color, label) = completion_label(failed);

        if should_space_before_totals(
            &self.command,
            self.verbose,
            changed,
            skipped,
            dry_run,
            failed,
        ) {
            self.task_result("");
        }
        self.always(&format!(
            "{text_color}\x1b[1m{label}\x1b[0m \x1b[2m\u{00b7} {elapsed_str}\x1b[0m"
        ));
        self.always(&status_line);
    }

    pub(in crate::logging) fn emit_recorded_task_result(&self, task_name: &str) {
        let task = self.tasks.lock().map_or(None, |guard| {
            guard
                .iter()
                .rev()
                .find(|task| task.name == task_name)
                .cloned()
        });
        let Some(task) = task else {
            return;
        };
        let details = self
            .task_details
            .lock()
            .map_or_else(|_| Vec::new(), |guard| guard.clone());

        let lines = task_result_lines(&task, &details);
        if lines.is_empty() {
            return;
        }

        self.separate_from_startup();
        for line in lines {
            self.task_result(&line);
        }
        self.mark_task_console_output();
    }
}

fn task_result_lines(task: &TaskEntry, details: &[TaskDetailEntry]) -> Vec<String> {
    if !should_emit_task_result(task.status) {
        return Vec::new();
    }

    let mut lines = vec![format_task_line(task)];
    for detail in task_detail_lines(details, task) {
        lines.push(format!("  {}", detail.trim_start()));
    }
    lines
}

const fn should_emit_task_result(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Changed | TaskStatus::Skipped | TaskStatus::DryRun | TaskStatus::Failed
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SummaryMode {
    Standard,
    Test,
}

impl SummaryMode {
    fn for_command(command: &str) -> Self {
        if command == "test" {
            Self::Test
        } else {
            Self::Standard
        }
    }
}

const fn completion_label(failed: u32) -> (&'static str, &'static str) {
    if failed > 0 {
        ("\x1b[31m", "Failed")
    } else {
        ("", "Complete")
    }
}

fn format_summary_counts(
    changed: u32,
    unchanged: u32,
    skipped: u32,
    dry_run: u32,
    failed: u32,
    mode: SummaryMode,
    show_dry_run_count: bool,
) -> String {
    let mut parts: Vec<String> = match mode {
        SummaryMode::Standard => vec![
            format!("\x1b[32m{changed} Changed\x1b[0m"),
            format!("\x1b[2m{unchanged} Unchanged\x1b[0m"),
        ],
        SummaryMode::Test => {
            let mut test_parts = vec![format!("\x1b[32m{changed} Passed\x1b[0m")];
            if unchanged > 0 {
                test_parts.push(format!("\x1b[2m{unchanged} Not run\x1b[0m"));
            }
            test_parts
        }
    };
    if skipped > 0 {
        parts.push(format!("\x1b[33m{skipped} Skipped\x1b[0m"));
    }
    if dry_run > 0 || show_dry_run_count {
        parts.push(format!("\x1b[35m{dry_run} Dry-run\x1b[0m"));
    }
    if failed > 0 {
        parts.push(format!("\x1b[31m{failed} Failed\x1b[0m"));
    }
    let separator = " \x1b[2m\u{00b7}\x1b[0m ";
    parts.join(separator)
}

fn should_space_before_totals(
    command: &str,
    verbose: bool,
    changed: u32,
    skipped: u32,
    dry_run: u32,
    failed: u32,
) -> bool {
    verbose
        || changed > 0
        || skipped > 0
        || dry_run > 0
        || failed > 0
        || !matches!(command, "install" | "update")
}

fn format_task_line(task: &TaskEntry) -> String {
    let Some(color) = task.status.color() else {
        return task.name.clone();
    };
    format!("{color}{}\x1b[0m", task.name)
}

fn task_detail_lines(details: &[TaskDetailEntry], task: &TaskEntry) -> Vec<String> {
    let task_message = task.message.as_deref();
    let lines = details
        .iter()
        .filter(|entry| entry.name == task.name)
        .flat_map(|entry| entry.lines.iter())
        .filter(|line| Some(line.as_str()) != task_message)
        .filter(|line| !is_prefixed_task_message(line, task_message))
        .filter(|line| !is_stats_summary(line))
        .cloned()
        .collect::<Vec<String>>();

    if !lines.is_empty() {
        return lines;
    }

    task.message
        .iter()
        .filter(|message| !is_stats_summary(message))
        .map(ToString::to_string)
        .collect()
}

fn is_prefixed_task_message(line: &str, task_message: Option<&str>) -> bool {
    let Some(message) = task_message else {
        return false;
    };
    ["skipped: ", "failed: ", "interrupted: "]
        .iter()
        .any(|prefix| line.strip_prefix(prefix) == Some(message))
}

fn is_stats_summary(line: &str) -> bool {
    let Some((first, rest)) = line.split_once(' ') else {
        return false;
    };
    first.parse::<u32>().is_ok()
        && (rest.starts_with("changed, ") || rest.starts_with("would change, "))
        && rest.contains(" already ok")
}

/// Format a duration as a human-readable string (e.g., "1.2s", "2m 5s").
fn format_elapsed(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        let mins = secs / 60;
        let remaining = secs % 60;
        format!("{mins}m {remaining}s")
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    #[test]
    fn format_elapsed_sub_second() {
        let d = Duration::from_millis(450);
        assert_eq!(format_elapsed(d), "0.5s");
    }

    #[test]
    fn format_elapsed_seconds() {
        let d = Duration::from_secs_f64(3.7);
        assert_eq!(format_elapsed(d), "3.7s");
    }

    #[test]
    fn format_elapsed_minutes() {
        let d = Duration::from_secs(125);
        assert_eq!(format_elapsed(d), "2m 5s");
    }

    #[test]
    fn format_summary_counts_uses_colored_text_without_symbols() {
        let summary = format_summary_counts(3, 17, 1, 0, 0, SummaryMode::Standard, false);
        assert_eq!(
            summary,
            "\x1b[32m3 Changed\x1b[0m \x1b[2m\u{00b7}\x1b[0m \x1b[2m17 Unchanged\x1b[0m \x1b[2m\u{00b7}\x1b[0m \x1b[33m1 Skipped\x1b[0m"
        );
        assert!(!summary.contains('\u{25cf}'));
        assert!(!summary.contains('\u{25cb}'));
        assert!(!summary.contains('\u{2717}'));
    }

    #[test]
    fn format_summary_counts_uses_test_terms_for_test_command() {
        let summary = format_summary_counts(6, 0, 0, 0, 1, SummaryMode::Test, false);
        assert_eq!(
            summary,
            "\x1b[32m6 Passed\x1b[0m \x1b[2m\u{00b7}\x1b[0m \x1b[31m1 Failed\x1b[0m"
        );
    }

    #[test]
    fn format_summary_counts_keeps_zero_dry_run_when_previewing() {
        let summary = format_summary_counts(0, 22, 1, 0, 0, SummaryMode::Standard, true);
        assert_eq!(
            summary,
            "\x1b[32m0 Changed\x1b[0m \x1b[2m\u{00b7}\x1b[0m \x1b[2m22 Unchanged\x1b[0m \x1b[2m\u{00b7}\x1b[0m \x1b[33m1 Skipped\x1b[0m \x1b[2m\u{00b7}\x1b[0m \x1b[35m0 Dry-run\x1b[0m"
        );
    }

    #[test]
    fn summary_totals_skip_extra_blank_for_non_verbose_no_op() {
        assert!(
            !should_space_before_totals("install", false, 0, 0, 0, 0),
            "install no-op runs should not separate the version and completion lines"
        );
        assert!(
            !should_space_before_totals("update", false, 0, 0, 0, 0),
            "update no-op runs should not separate the version and completion lines"
        );
    }

    #[test]
    fn summary_totals_keep_separator_when_output_was_visible() {
        assert!(should_space_before_totals("install", false, 1, 0, 0, 0));
        assert!(should_space_before_totals("install", false, 0, 1, 0, 0));
        assert!(should_space_before_totals("install", false, 0, 0, 1, 0));
        assert!(should_space_before_totals("install", false, 0, 0, 0, 1));
        assert!(should_space_before_totals("install", true, 0, 0, 0, 0));
        assert!(should_space_before_totals("test", false, 0, 0, 0, 0));
    }

    #[test]
    fn format_task_line_uses_no_leading_indent() {
        let task = TaskEntry {
            name: "symlinks".to_string(),
            status: TaskStatus::Changed,
            message: Some("3 changed, 8 already ok".to_string()),
        };

        assert_eq!(format_task_line(&task), "\x1b[32msymlinks\x1b[0m");
    }

    #[test]
    fn task_detail_lines_filters_generic_stats_summary() {
        let task = TaskEntry {
            name: "symlinks".to_string(),
            status: TaskStatus::Changed,
            message: Some("2 changed, 1 already ok".to_string()),
        };
        let details = vec![TaskDetailEntry {
            name: "symlinks".to_string(),
            lines: vec![
                "linked: ~/.bashrc".to_string(),
                "2 changed, 1 already ok".to_string(),
            ],
        }];

        assert_eq!(
            task_detail_lines(&details, &task),
            vec!["linked: ~/.bashrc"]
        );
    }

    #[test]
    fn task_detail_lines_filters_prefixed_skip_message() {
        let task = TaskEntry {
            name: "skip-task".to_string(),
            status: TaskStatus::Skipped,
            message: Some("dependency failed".to_string()),
        };
        let details = vec![TaskDetailEntry {
            name: "skip-task".to_string(),
            lines: vec!["skipped: dependency failed".to_string()],
        }];

        assert_eq!(
            task_detail_lines(&details, &task),
            vec!["dependency failed"]
        );
    }

    #[test]
    fn task_detail_lines_keeps_custom_message_when_no_details_exist() {
        let task = TaskEntry {
            name: "custom task".to_string(),
            status: TaskStatus::Changed,
            message: Some("generated private config".to_string()),
        };

        assert_eq!(
            task_detail_lines(&[], &task),
            vec!["generated private config"]
        );
    }

    #[test]
    fn task_result_lines_are_flat_with_reduced_indent() {
        let task = TaskEntry {
            name: "changed-task".to_string(),
            status: TaskStatus::Changed,
            message: None,
        };
        let details = vec![TaskDetailEntry {
            name: "changed-task".to_string(),
            lines: vec!["linked: ~/.example".to_string()],
        }];

        assert_eq!(
            task_result_lines(&task, &details),
            vec!["\x1b[32mchanged-task\x1b[0m", "  linked: ~/.example",]
        );
    }

    #[test]
    fn task_result_lines_skip_unchanged_tasks() {
        let task = TaskEntry {
            name: "unchanged-task".to_string(),
            status: TaskStatus::Ok,
            message: None,
        };

        assert!(task_result_lines(&task, &[]).is_empty());
    }

    #[test]
    fn print_summary_clears_visible_progress() {
        let (log, _tmp, _guard) = crate::logging::isolated_logger();
        log.record_task("changed-task", TaskStatus::Changed, None);
        log.notify_task_start_with_progress("active-task", true);

        assert!(log.has_transient_rows());
        assert!(log.has_status_row());

        log.print_summary();

        assert!(!log.has_transient_rows());
        assert!(!log.has_status_row());
    }
}
