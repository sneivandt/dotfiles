//! Per-task line rendering for the end-of-run summary.
//!
//! Converts a recorded task plus its buffered detail lines into the console
//! rows shown beneath (or in place of) the aggregate totals.

use super::totals::SummaryMode;
use crate::infra::logging::logger::TaskDetailEntry;
use crate::infra::logging::style::{StyleChoice, TextStyle};
use crate::infra::logging::types::{TaskEntry, TaskStatus};

pub(super) fn task_result_lines(
    task: &TaskEntry,
    details: &[TaskDetailEntry],
    mode: SummaryMode,
    style: StyleChoice,
) -> Vec<String> {
    if !should_emit_task_result(task.status) {
        return Vec::new();
    }

    let mut lines = vec![format_task_line(task, mode, style)];
    let details = task_detail_lines(details, task);
    let candidates: Vec<&str> = details
        .iter()
        .flat_map(|detail| detail.lines())
        .filter(|line| !line.trim().is_empty())
        .filter(|line| !is_stats_summary(line))
        .collect();
    for detail in candidates {
        let compacted = compact_detail_line(detail);
        lines.push(style.paint(TextStyle::Dim, &format!("  {}", compacted.trim_start())));
    }
    lines
}

pub(super) fn compact_detail_line(line: &str) -> String {
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
            return format!("{verb} {detail}");
        }
    }
    for verb in ["configure", "install", "link", "remove", "update"] {
        if let Some(detail) = line
            .strip_prefix(verb)
            .and_then(|rest| rest.strip_prefix(' '))
        {
            return format!("{verb} {detail}");
        }
    }
    line.to_string()
}

pub(super) const fn should_emit_task_result(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Changed
            | TaskStatus::Passed
            | TaskStatus::Skipped
            | TaskStatus::DryRun
            | TaskStatus::Failed
    )
}

pub(super) fn format_task_line(task: &TaskEntry, mode: SummaryMode, style: StyleChoice) -> String {
    let Some(text_style) = task.status.text_style() else {
        return task.name.clone();
    };
    let status = match task.status {
        TaskStatus::Changed => {
            if mode == SummaryMode::Test {
                "PASSED"
            } else {
                "CHANGE"
            }
        }
        TaskStatus::Passed => "PASSED",
        TaskStatus::DryRun => "DRYRUN",
        TaskStatus::Skipped => "IGNORE",
        TaskStatus::Failed => "FAILED",
        TaskStatus::Ok | TaskStatus::NotApplicable => return task.name.clone(),
    };
    format!("{} {}", style.paint(text_style, status), task.name)
}

pub(super) fn task_detail_lines(details: &[TaskDetailEntry], task: &TaskEntry) -> Vec<String> {
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

pub(super) fn is_prefixed_task_message(line: &str, task_message: Option<&str>) -> bool {
    let Some(message) = task_message else {
        return false;
    };
    ["skipped: ", "failed: ", "interrupted: "]
        .iter()
        .any(|prefix| line.strip_prefix(prefix) == Some(message))
}

pub(super) fn is_stats_summary(line: &str) -> bool {
    let Some((first, rest)) = line.split_once(' ') else {
        return false;
    };
    first.parse::<u32>().is_ok()
        && (rest.starts_with("changed, ") || rest.starts_with("would change, "))
        && rest.contains(" already ok")
}
