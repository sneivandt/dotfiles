//! End-of-run summary printing for [`Logger`].
//!
//! Renders final aggregate counts and compact completed-task rows.

use std::time::Duration;

use super::{Logger, TaskDetailEntry};
use crate::infra::logging::style::{StyleChoice, TextStyle, stdout_style};
use crate::infra::logging::types::{ActionCounts, TaskEntry, TaskStatus};

const MAX_NON_VERBOSE_DETAIL_LINES: usize = 8;

impl Logger {
    /// Print the summary of all recorded tasks.
    pub fn print_summary(&self) {
        let tasks = match self.tasks.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        self.clear_status();

        let elapsed = self.start.elapsed();
        let elapsed_str = format_elapsed(elapsed);

        let summary_mode = SummaryMode::for_command(&self.command);
        let counts = SummaryCounts::from_tasks(&tasks);
        let style = stdout_style();

        if should_space_before_totals(&self.command, self.verbose, counts.has_visible_tasks()) {
            self.task_result("");
        }
        for line in format_summary_lines(counts, summary_mode, self.dry_run, &elapsed_str, style) {
            self.always(&line);
        }
    }

    pub(in crate::infra::logging) fn emit_recorded_task_result(&self, task_name: &str) {
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

        let lines = task_result_lines(
            &task,
            &details,
            SummaryMode::for_command(&self.command),
            stdout_style(),
        );
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

fn task_result_lines(
    task: &TaskEntry,
    details: &[TaskDetailEntry],
    mode: SummaryMode,
    style: StyleChoice,
) -> Vec<String> {
    if !should_emit_task_result(task.status) {
        return Vec::new();
    }

    let mut lines = vec![format_task_line(task, mode, style)];
    let detail_lines = task_detail_lines(details, task)
        .iter()
        .flat_map(|detail| detail.lines())
        .filter(|line| !line.trim().is_empty())
        .filter(|line| !is_stats_summary(line))
        .map(compact_detail_line)
        .collect::<Vec<String>>();
    for detail in detail_lines.iter().take(MAX_NON_VERBOSE_DETAIL_LINES) {
        lines.push(format!("  {}", detail.trim_start()));
    }

    let remaining = detail_lines
        .len()
        .saturating_sub(MAX_NON_VERBOSE_DETAIL_LINES);
    if remaining > 0 {
        lines.push(format!(
            "  \u{2026} {remaining} more; use -v for the full plan"
        ));
    }
    lines
}

fn compact_detail_line(line: &str) -> String {
    const ACTION_PREFIXES: &[(&str, &str)] = &[
        ("would configure: ", "configure"),
        ("would install: ", "install"),
        ("would link: ", "link"),
        ("would remove: ", "remove"),
        ("would update: ", "update"),
        ("configured: ", "configure"),
        ("installed: ", "install"),
        ("linked: ", "link"),
        ("removed: ", "remove"),
        ("updated: ", "update"),
    ];

    let line = line.trim_start();
    for (prefix, verb) in ACTION_PREFIXES {
        if let Some(detail) = line.strip_prefix(prefix) {
            let target = detail
                .split_once(" \u{2190} ")
                .or_else(|| detail.split_once(" -> "))
                .map_or(detail, |(target, _)| target);
            return format!("{verb} {target}");
        }
    }
    for verb in ["configure", "install", "link", "remove", "update"] {
        if let Some(detail) = line
            .strip_prefix(verb)
            .and_then(|rest| rest.strip_prefix(' '))
        {
            let target = detail
                .split_once(" \u{2190} ")
                .or_else(|| detail.split_once(" -> "))
                .map_or(detail, |(target, _)| target);
            return format!("{verb} {target}");
        }
    }
    line.to_string()
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SummaryCounts {
    changed: u32,
    skipped: u32,
    dry_run: u32,
    failed: u32,
    actions: ActionCounts,
}

impl SummaryCounts {
    fn from_tasks(tasks: &[TaskEntry]) -> Self {
        let mut counts = Self::default();
        for task in tasks {
            match task.status {
                TaskStatus::Changed => counts.changed = counts.changed.saturating_add(1),
                TaskStatus::Ok | TaskStatus::NotApplicable => {}
                TaskStatus::Skipped => counts.skipped = counts.skipped.saturating_add(1),
                TaskStatus::DryRun => counts.dry_run = counts.dry_run.saturating_add(1),
                TaskStatus::Failed => counts.failed = counts.failed.saturating_add(1),
            }
            counts.actions.merge(task.actions);
        }
        counts
    }

    const fn has_visible_tasks(self) -> bool {
        self.changed > 0 || self.skipped > 0 || self.dry_run > 0 || self.failed > 0
    }
}

fn format_summary_lines(
    counts: SummaryCounts,
    mode: SummaryMode,
    dry_run: bool,
    elapsed: &str,
    style: StyleChoice,
) -> Vec<String> {
    if mode == SummaryMode::Standard && !counts.has_visible_tasks() {
        return vec![format_completion_line(
            "Already up to date",
            None,
            elapsed,
            style,
        )];
    }

    let (label, label_style) = if counts.failed > 0 {
        ("Failed", Some(TextStyle::Red))
    } else if mode == SummaryMode::Standard && dry_run {
        ("Preview complete", None)
    } else {
        ("Complete", None)
    };
    let mut lines = vec![format_completion_line(label, label_style, elapsed, style)];

    let totals = match mode {
        SummaryMode::Standard => format_standard_totals(counts, style),
        SummaryMode::Test => format_test_totals(counts, style),
    };
    if let Some(totals) = totals {
        lines.push(totals);
    }
    lines
}

fn format_completion_line(
    label: &str,
    label_style: Option<TextStyle>,
    elapsed: &str,
    style: StyleChoice,
) -> String {
    format!(
        "{} {}",
        label_style.map_or_else(
            || label.to_string(),
            |text_style| style.paint(text_style, label)
        ),
        style.paint(TextStyle::Dim, &format!("\u{00b7} {elapsed}"))
    )
}

fn format_standard_totals(counts: SummaryCounts, style: StyleChoice) -> Option<String> {
    let mut task_parts = Vec::new();
    if counts.changed > 0 {
        task_parts.push(style.paint(TextStyle::Green, &format!("{} changed", counts.changed)));
    }
    if counts.dry_run > 0 {
        task_parts.push(style.paint(
            TextStyle::Magenta,
            &format!("{} would change", counts.dry_run),
        ));
    }
    if counts.skipped > 0 {
        task_parts.push(style.paint(TextStyle::Yellow, &format!("{} skipped", counts.skipped)));
    }
    if counts.failed > 0 {
        task_parts.push(style.paint(TextStyle::Red, &format!("{} failed", counts.failed)));
    }

    let mut groups = Vec::new();
    if !task_parts.is_empty() {
        groups.push(format!("{} {}", "Tasks:", task_parts.join(", ")));
    }
    if !counts.actions.is_empty() {
        groups.push(format_action_totals(counts.actions, style));
    }
    if groups.is_empty() {
        None
    } else {
        Some(groups.join(&format!(" {} ", style.paint(TextStyle::Dim, "\u{00b7}"))))
    }
}

fn format_action_totals(counts: ActionCounts, style: StyleChoice) -> String {
    let mut parts = Vec::new();
    if counts.applied > 0 {
        parts.push(style.paint(TextStyle::Green, &format!("{} applied", counts.applied)));
    }
    if counts.planned > 0 {
        parts.push(style.paint(TextStyle::Magenta, &format!("{} planned", counts.planned)));
    }
    if counts.skipped > 0 {
        parts.push(style.paint(TextStyle::Yellow, &format!("{} skipped", counts.skipped)));
    }
    if counts.failed > 0 {
        parts.push(style.paint(TextStyle::Red, &format!("{} failed", counts.failed)));
    }
    format!("{} {}", "Actions:", parts.join(", "))
}

fn format_test_totals(counts: SummaryCounts, style: StyleChoice) -> Option<String> {
    let mut parts = Vec::new();
    if counts.changed > 0 {
        parts.push(style.paint(TextStyle::Green, &format!("{} passed", counts.changed)));
    }
    if counts.skipped > 0 {
        parts.push(style.paint(TextStyle::Yellow, &format!("{} skipped", counts.skipped)));
    }
    if counts.failed > 0 {
        parts.push(style.paint(TextStyle::Red, &format!("{} failed", counts.failed)));
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("{} {}", "Checks:", parts.join(", ")))
    }
}

fn should_space_before_totals(command: &str, verbose: bool, has_visible_tasks: bool) -> bool {
    verbose || has_visible_tasks || !matches!(command, "install" | "update" | "uninstall")
}

fn format_task_line(task: &TaskEntry, mode: SummaryMode, style: StyleChoice) -> String {
    let Some(text_style) = task.status.text_style() else {
        return task.name.clone();
    };
    let status = match task.status {
        TaskStatus::Changed if mode == SummaryMode::Test => "passed",
        TaskStatus::Changed => "changed",
        TaskStatus::DryRun => "would change",
        TaskStatus::Skipped => "skipped",
        TaskStatus::Failed => "failed",
        TaskStatus::Ok | TaskStatus::NotApplicable => return task.name.clone(),
    };
    format!(
        "{} {} {}",
        task.name,
        style.paint(TextStyle::Dim, "\u{00b7}"),
        style.paint(text_style, status)
    )
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
    fn format_elapsed_values() {
        assert_eq!(format_elapsed(Duration::from_millis(450)), "0.5s");
        assert_eq!(format_elapsed(Duration::from_secs_f64(3.7)), "3.7s");
        assert_eq!(format_elapsed(Duration::from_secs(125)), "2m 5s");
    }

    #[test]
    fn standard_no_op_has_only_already_up_to_date_line() {
        let lines = format_summary_lines(
            SummaryCounts::default(),
            SummaryMode::Standard,
            false,
            "1.2s",
            StyleChoice::plain(),
        );

        assert_eq!(lines, ["Already up to date · 1.2s"]);
    }

    #[test]
    fn standard_summary_groups_task_and_action_counts() {
        let lines = format_summary_lines(
            SummaryCounts {
                changed: 3,
                skipped: 1,
                dry_run: 0,
                failed: 1,
                actions: ActionCounts {
                    applied: 87,
                    planned: 0,
                    skipped: 2,
                    failed: 1,
                },
            },
            SummaryMode::Standard,
            false,
            "2.0s",
            StyleChoice::plain(),
        );

        assert_eq!(
            lines,
            [
                "Failed · 2.0s",
                "Tasks: 3 changed, 1 skipped, 1 failed · Actions: 87 applied, 2 skipped, 1 failed",
            ]
        );
        assert!(
            !lines
                .get(1)
                .expect("summary totals should exist")
                .contains("unchanged")
        );
    }

    #[test]
    fn preview_summary_uses_planned_vocabulary() {
        let lines = format_summary_lines(
            SummaryCounts {
                changed: 0,
                skipped: 0,
                dry_run: 1,
                failed: 0,
                actions: ActionCounts {
                    planned: 81,
                    ..ActionCounts::default()
                },
            },
            SummaryMode::Standard,
            true,
            "0.8s",
            StyleChoice::plain(),
        );

        assert_eq!(
            lines,
            [
                "Preview complete · 0.8s",
                "Tasks: 1 would change · Actions: 81 planned",
            ]
        );
    }

    #[test]
    fn standard_summary_omits_actions_when_all_action_counts_are_zero() {
        let lines = format_summary_lines(
            SummaryCounts {
                changed: 2,
                skipped: 0,
                dry_run: 0,
                failed: 0,
                actions: ActionCounts::default(),
            },
            SummaryMode::Standard,
            false,
            "1.0s",
            StyleChoice::plain(),
        );

        assert_eq!(lines, ["Complete · 1.0s", "Tasks: 2 changed"]);
    }

    #[test]
    fn test_summary_uses_check_vocabulary_and_omits_not_run() {
        let lines = format_summary_lines(
            SummaryCounts {
                changed: 7,
                skipped: 2,
                dry_run: 0,
                failed: 1,
                actions: ActionCounts::default(),
            },
            SummaryMode::Test,
            false,
            "3.4s",
            StyleChoice::plain(),
        );

        assert_eq!(
            lines,
            ["Failed · 3.4s", "Checks: 7 passed, 2 skipped, 1 failed"]
        );
        assert!(
            !lines
                .get(1)
                .expect("check totals should exist")
                .contains("not run")
        );
    }

    #[test]
    fn no_op_standard_commands_skip_extra_blank() {
        for command in ["install", "update", "uninstall"] {
            assert!(
                !should_space_before_totals(command, false, false),
                "{command} no-op runs should not add an extra separator"
            );
        }
        assert!(should_space_before_totals("install", false, true));
        assert!(should_space_before_totals("install", true, false));
        assert!(should_space_before_totals("test", false, false));
    }

    #[test]
    fn task_line_colors_only_explicit_status_text() {
        let task = TaskEntry {
            name: "symlinks".to_string(),
            status: TaskStatus::Changed,
            message: Some("3 changed, 8 already ok".to_string()),
            actions: ActionCounts::default(),
        };

        assert_eq!(
            format_task_line(&task, SummaryMode::Standard, StyleChoice::colored()),
            "symlinks \x1b[2m·\x1b[0m \x1b[32mchanged\x1b[0m"
        );
        assert_eq!(
            format_task_line(&task, SummaryMode::Standard, StyleChoice::plain()),
            "symlinks · changed"
        );
    }

    #[test]
    fn task_detail_lines_filters_generic_stats_summary() {
        let task = TaskEntry {
            name: "symlinks".to_string(),
            status: TaskStatus::Changed,
            message: Some("2 changed, 1 already ok".to_string()),
            actions: ActionCounts::default(),
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
            actions: ActionCounts::default(),
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
            actions: ActionCounts::default(),
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
            actions: ActionCounts::default(),
        };
        let details = vec![TaskDetailEntry {
            name: "changed-task".to_string(),
            lines: vec!["linked: ~/.example".to_string()],
        }];

        assert_eq!(
            task_result_lines(
                &task,
                &details,
                SummaryMode::Standard,
                StyleChoice::colored(),
            ),
            vec![
                "changed-task \x1b[2m·\x1b[0m \x1b[32mchanged\x1b[0m",
                "  link ~/.example",
            ]
        );
    }

    #[test]
    fn task_result_lines_abbreviate_symlink_actions() {
        let task = TaskEntry {
            name: "Install symlinks".to_string(),
            status: TaskStatus::DryRun,
            message: None,
            actions: ActionCounts {
                planned: 1,
                ..ActionCounts::default()
            },
        };
        let details = vec![TaskDetailEntry {
            name: task.name.clone(),
            lines: vec!["would link: ~/.bashrc \u{2190} symlinks/bashrc".to_string()],
        }];

        assert_eq!(
            task_result_lines(&task, &details, SummaryMode::Standard, StyleChoice::plain(),),
            ["Install symlinks · would change", "  link ~/.bashrc"]
        );
    }

    #[test]
    fn task_result_lines_bound_non_verbose_details() {
        let task = TaskEntry {
            name: "large-plan".to_string(),
            status: TaskStatus::DryRun,
            message: None,
            actions: ActionCounts::default(),
        };
        let details = vec![TaskDetailEntry {
            name: "large-plan".to_string(),
            lines: vec![
                (1..=11)
                    .map(|index| format!("item {index}"))
                    .collect::<Vec<String>>()
                    .join("\n"),
            ],
        }];

        let lines = task_result_lines(&task, &details, SummaryMode::Standard, StyleChoice::plain());

        assert_eq!(lines.len(), 10);
        assert_eq!(
            lines.first().expect("task status line should exist"),
            "large-plan · would change"
        );
        assert_eq!(
            lines.get(8).expect("eighth detail line should exist"),
            "  item 8"
        );
        assert_eq!(
            lines.get(9).expect("overflow detail line should exist"),
            "  … 3 more; use -v for the full plan"
        );
    }

    #[test]
    fn task_result_lines_skip_unchanged_tasks() {
        let task = TaskEntry {
            name: "unchanged-task".to_string(),
            status: TaskStatus::Ok,
            message: None,
            actions: ActionCounts::default(),
        };

        assert!(
            task_result_lines(&task, &[], SummaryMode::Standard, StyleChoice::colored()).is_empty()
        );
    }

    #[test]
    fn validation_task_line_uses_passed_status() {
        let task = TaskEntry {
            name: "Validate config".to_string(),
            status: TaskStatus::Changed,
            message: None,
            actions: ActionCounts::default(),
        };

        assert_eq!(
            format_task_line(&task, SummaryMode::Test, StyleChoice::plain()),
            "Validate config · passed"
        );
    }

    #[test]
    fn colored_summary_does_not_bold_completion_or_group_labels() {
        let lines = format_summary_lines(
            SummaryCounts {
                changed: 1,
                actions: ActionCounts {
                    applied: 2,
                    ..ActionCounts::default()
                },
                ..SummaryCounts::default()
            },
            SummaryMode::Standard,
            false,
            "1.0s",
            StyleChoice::colored(),
        );

        assert!(
            lines.iter().all(|line| !line.contains("\x1b[1m")),
            "completion and totals labels should not be bold"
        );
    }

    #[test]
    fn print_summary_clears_visible_progress() {
        let (log, _tmp, _guard) = crate::infra::logging::isolated_logger();
        log.record_task("changed-task", TaskStatus::Changed, None);
        log.notify_task_start_with_progress("active-task", true);

        assert!(log.has_transient_rows());
        assert!(log.has_status_row());

        log.print_summary();

        assert!(!log.has_transient_rows());
        assert!(!log.has_status_row());
    }
}
