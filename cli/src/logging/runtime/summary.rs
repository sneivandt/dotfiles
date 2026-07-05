//! End-of-run summary printing for [`Logger`].
//!
//! Renders grouped task-result sections and final aggregate counts.

use std::time::Duration;

use super::{Logger, TaskDetailEntry};
use crate::logging::types::{TaskEntry, TaskStatus};

impl Logger {
    /// Print the summary of all recorded tasks.
    pub fn print_summary(&self) {
        self.clear_status();
        let tasks = match self.tasks.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        if tasks.is_empty() {
            return;
        }

        let details = self
            .task_details
            .lock()
            .map_or_else(|_| Vec::new(), |guard| guard.clone());

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

        let mut emitted_task_section = false;
        for spec in task_section_specs(summary_mode) {
            let emitted = print_task_section(
                spec.title,
                &tasks,
                &details,
                emitted_task_section,
                |task| task.status == spec.status,
                |line| self.task_result(line),
            );
            emitted_task_section = emitted || emitted_task_section;
        }

        if should_space_before_totals(self.verbose, changed, skipped, dry_run, failed) {
            self.task_result("");
        }
        self.always(&format!(
            "{text_color}\x1b[1m{label}\x1b[0m \x1b[2m\u{00b7} {elapsed_str}\x1b[0m"
        ));
        self.always(&status_line);
    }

    pub(in crate::logging) fn live_task_section_lines(&self) -> Vec<String> {
        let tasks = self
            .tasks
            .lock()
            .map_or_else(|_| Vec::new(), |guard| guard.clone());
        let details = self
            .task_details
            .lock()
            .map_or_else(|_| Vec::new(), |guard| guard.clone());
        task_section_lines(&tasks, &details, SummaryMode::for_command(&self.command))
    }
}

fn task_section_lines(
    tasks: &[TaskEntry],
    details: &[TaskDetailEntry],
    mode: SummaryMode,
) -> Vec<String> {
    let mut lines = Vec::new();
    let mut emitted_task_section = false;
    for spec in task_section_specs(mode) {
        let emitted = print_task_section(
            spec.title,
            tasks,
            details,
            emitted_task_section,
            |task| task.status == spec.status,
            |line| lines.push(line.to_string()),
        );
        emitted_task_section = emitted || emitted_task_section;
    }
    lines
}

fn print_task_section(
    title: &str,
    tasks: &[TaskEntry],
    details: &[TaskDetailEntry],
    leading_blank: bool,
    include: impl Fn(&TaskEntry) -> bool,
    mut emit: impl FnMut(&str),
) -> bool {
    let section_tasks: Vec<&TaskEntry> = tasks.iter().filter(|task| include(task)).collect();
    if section_tasks.is_empty() {
        return false;
    }

    if leading_blank {
        emit("");
    }
    emit(&format!("\x1b[1m{title}\x1b[0m"));
    for task in section_tasks {
        emit(&format_task_line(task));
        for detail in task_detail_lines(details, task) {
            emit(&format!("    {}", detail.trim_start()));
        }
    }
    true
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TaskSectionSpec {
    title: &'static str,
    status: TaskStatus,
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

const fn task_section_specs(mode: SummaryMode) -> [TaskSectionSpec; 4] {
    let changed_title = match mode {
        SummaryMode::Standard => "Changed",
        SummaryMode::Test => "Tests",
    };
    [
        TaskSectionSpec {
            title: changed_title,
            status: TaskStatus::Changed,
        },
        TaskSectionSpec {
            title: "Skipped",
            status: TaskStatus::Skipped,
        },
        TaskSectionSpec {
            title: "Failed",
            status: TaskStatus::Failed,
        },
        TaskSectionSpec {
            title: "Dry-run",
            status: TaskStatus::DryRun,
        },
    ]
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

const fn should_space_before_totals(
    verbose: bool,
    changed: u32,
    skipped: u32,
    dry_run: u32,
    failed: u32,
) -> bool {
    verbose || changed > 0 || skipped > 0 || dry_run > 0 || failed > 0
}

fn format_task_line(task: &TaskEntry) -> String {
    let Some(color) = task.status.color() else {
        return format!("  {}", task.name);
    };
    format!("{color}  {}\x1b[0m", task.name)
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
    fn task_section_specs_include_skipped_before_failed() {
        let specs = task_section_specs(SummaryMode::Standard);

        assert_eq!(
            specs,
            [
                TaskSectionSpec {
                    title: "Changed",
                    status: TaskStatus::Changed,
                },
                TaskSectionSpec {
                    title: "Skipped",
                    status: TaskStatus::Skipped,
                },
                TaskSectionSpec {
                    title: "Failed",
                    status: TaskStatus::Failed,
                },
                TaskSectionSpec {
                    title: "Dry-run",
                    status: TaskStatus::DryRun,
                },
            ]
        );
    }

    #[test]
    fn summary_totals_skip_extra_blank_for_non_verbose_no_op() {
        assert!(
            !should_space_before_totals(false, 0, 0, 0, 0),
            "non-verbose no-op runs already have the header separator"
        );
    }

    #[test]
    fn summary_totals_keep_separator_when_output_was_visible() {
        assert!(should_space_before_totals(false, 1, 0, 0, 0));
        assert!(should_space_before_totals(false, 0, 1, 0, 0));
        assert!(should_space_before_totals(false, 0, 0, 1, 0));
        assert!(should_space_before_totals(false, 0, 0, 0, 1));
        assert!(should_space_before_totals(true, 0, 0, 0, 0));
    }

    #[test]
    fn format_task_line_includes_changed_message() {
        let task = TaskEntry {
            name: "symlinks".to_string(),
            status: TaskStatus::Changed,
            message: Some("3 changed, 8 already ok".to_string()),
        };

        assert_eq!(format_task_line(&task), "\x1b[32m  symlinks\x1b[0m");
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
    fn task_section_lines_match_summary_grouping_for_live_status() {
        let tasks = vec![
            TaskEntry {
                name: "changed-task".to_string(),
                status: TaskStatus::Changed,
                message: None,
            },
            TaskEntry {
                name: "skipped-task".to_string(),
                status: TaskStatus::Skipped,
                message: Some("not needed".to_string()),
            },
        ];
        let details = vec![TaskDetailEntry {
            name: "changed-task".to_string(),
            lines: vec!["linked: ~/.example".to_string()],
        }];

        assert_eq!(
            task_section_lines(&tasks, &details, SummaryMode::Standard),
            vec![
                "\x1b[1mChanged\x1b[0m",
                "\x1b[32m  changed-task\x1b[0m",
                "    linked: ~/.example",
                "",
                "\x1b[1mSkipped\x1b[0m",
                "\x1b[33m  skipped-task\x1b[0m",
                "    not needed",
            ]
        );
    }
}
