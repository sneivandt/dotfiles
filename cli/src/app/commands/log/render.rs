use super::{HashMap, HashSet, Record, Result, StyleChoice, TaskStatus, TextStyle};
use anyhow::Context as _;

// Keep both decoded record payloads and legacy text safe for text tools.
// Raw viewing and task matching use the original bytes and identities.
pub(super) fn write_human_line(out: &mut dyn std::io::Write, line: &str) -> Result<()> {
    if line.contains('\0') {
        writeln!(out, "{}", line.replace('\0', r"\0"))
    } else {
        writeln!(out, "{line}")
    }
    .context("writing log output")
}

pub(super) fn write_rendered_record(
    out: &mut dyn std::io::Write,
    context: &str,
    task_names: &HashMap<String, String>,
    event: &str,
    message: &str,
    original_line: Option<&str>,
    style: StyleChoice,
) -> Result<()> {
    if let Some(line) = original_line {
        let prefix = line
            .split_once(" [record] ")
            .map_or("", |(prefix, _)| prefix);
        let event_badge = style.paint(verbose_event_style(event), &format!("[{event}]"));
        return write_human_line(
            out,
            &format!(
                "{} {event_badge} {message}",
                style.paint(TextStyle::Dim, prefix)
            ),
        );
    }
    let label = match event {
        "run_start" => "Run",
        "run_context" => "Configuration",
        "run_scope" => "Selection",
        "run_finish" | "task_done" | "resource_result" => "Result",
        "warn" | "warn_continuation" => "Warning",
        "error" => "Error",
        "command" => "Command",
        "plan" | "dry_run" => "Planned",
        "task_skip" => "Skipped",
        "info" | "startup" | "action" | "resource_apply" | "resource_remove" | "task_result" => "",
        other => other,
    };
    let mut labels = Vec::new();
    if context != "main" && event != "task_result" {
        let task_name = task_names.get(context).map_or_else(
            || context.rsplit("::").next().unwrap_or(context),
            String::as_str,
        );
        labels.push(style.bold(task_name));
    }
    if !label.is_empty() {
        labels.push(style.paint(label_style(event), label));
    }
    let body = style_record_message(event, message, style);
    if labels.is_empty() {
        write_human_line(out, &body)
    } else {
        let sep = style.paint(TextStyle::Dim, " · ");
        let colon = style.paint(TextStyle::Dim, ": ");
        write_human_line(out, &format!("{}{colon}{body}", labels.join(&sep)))
    }
}

fn label_style(event: &str) -> TextStyle {
    match event {
        "warn" | "warn_continuation" | "task_skip" => TextStyle::Yellow,
        "error" => TextStyle::Red,
        "plan" | "dry_run" => TextStyle::Magenta,
        _ => TextStyle::Dim,
    }
}

fn verbose_event_style(event: &str) -> TextStyle {
    match event {
        "warn" | "warn_continuation" | "task_skip" => TextStyle::Yellow,
        "error" => TextStyle::Red,
        "action" | "resource_apply" | "resource_remove" => TextStyle::Cyan,
        "plan" | "dry_run" => TextStyle::Magenta,
        _ => TextStyle::Dim,
    }
}

fn style_record_message(event: &str, message: &str, style: StyleChoice) -> String {
    match event {
        "task_result" => style_task_result(message, style),
        "run_finish" => style_run_finish(message, style),
        "startup" | "run_context" => style_dot_separated_header(message, style),
        "action" | "plan" => style_action_message(message, event == "plan", style),
        _ => message.to_string(),
    }
}

fn style_task_result(message: &str, style: StyleChoice) -> String {
    let Some((name, rest)) = message.split_once(": ") else {
        return message.to_string();
    };
    let (status, reason) = rest
        .split_once(": ")
        .map_or((rest, None), |(s, r)| (s, Some(r)));
    let status_tone = match status {
        "Changed" | "Passed" => TextStyle::Green,
        "Planned" => TextStyle::Magenta,
        "Skipped" | "Blocked" | "Interrupted" => TextStyle::Yellow,
        "Failed" => TextStyle::Red,
        _ => TextStyle::Dim,
    };
    let colon = style.paint(TextStyle::Dim, ": ");
    let mut rendered = format!(
        "{}{colon}{}",
        style.bold(name),
        style.paint(status_tone, status)
    );
    if let Some(reason) = reason {
        rendered.push_str(&colon);
        rendered.push_str(reason);
    }
    rendered
}

fn style_run_finish(message: &str, style: StyleChoice) -> String {
    let Some((outcome, rest)) = message.split_once(" · ") else {
        return message.to_string();
    };
    let outcome_tone = match outcome {
        "succeeded" => TextStyle::Green,
        "failed" => TextStyle::Red,
        "interrupted" => TextStyle::Yellow,
        _ => TextStyle::Dim,
    };
    format!(
        "{}{}",
        style.paint(outcome_tone, outcome),
        style.paint(TextStyle::Dim, &format!(" · {rest}"))
    )
}

fn style_dot_separated_header(message: &str, style: StyleChoice) -> String {
    let Some((lead, rest)) = message.split_once(" · ") else {
        return message.to_string();
    };
    let mut out = style.bold(lead);
    for part in rest.split(" · ") {
        out.push_str(&style.paint(TextStyle::Dim, " · "));
        let tone = if part == "dry run" {
            TextStyle::Magenta
        } else {
            TextStyle::Dim
        };
        out.push_str(&style.paint(tone, part));
    }
    out
}

fn style_action_message(message: &str, planned: bool, style: StyleChoice) -> String {
    let Some((verb, rest)) = message.split_once(' ') else {
        return message.to_string();
    };
    let verb_tone = if planned {
        TextStyle::Magenta
    } else {
        TextStyle::Cyan
    };
    let formatted_rest = match rest.split_once(" \u{2192} ") {
        Some((left, right)) => {
            format!("{left}{}{right}", style.paint(TextStyle::Dim, " \u{2192} "))
        }
        None => rest.to_string(),
    };
    format!("{} {formatted_rest}", style.paint(verb_tone, verb))
}

pub(super) fn text_record_parts(line: &str) -> Option<(&str, &str, &str)> {
    let (context, tail) = line.split_once(" [")?.1.split_once("] [")?;
    let (event, message) = tail.split_once("] ")?;
    matches!(
        event,
        "info"
            | "startup"
            | "warn"
            | "warn_continuation"
            | "error"
            | "action"
            | "dry_run"
            | "task_done"
            | "task_skip"
            | "resource_apply"
            | "resource_remove"
            | "resource_result"
    )
    .then_some((context, event, message))
}

/// Plumbing remains available in verbose/raw viewing, including legacy logs.
pub(super) fn diagnostic_event(event: &str) -> bool {
    matches!(
        event,
        "debug"
            | "trace"
            | "stage"
            | "task_wait"
            | "task_start"
            | "task_timing"
            | "resource_check"
            | "summary"
    )
}

pub(super) fn legacy_duplicate(
    line: &str,
    context: Option<&str>,
    results: &HashSet<String>,
    durations: &HashSet<String>,
) -> bool {
    let Some(context) = context else {
        return false;
    };
    match line_event(line) {
        Some("task_done" | "task_skip") => results.contains(context),
        Some("task_timing") => durations.contains(context),
        _ => false,
    }
}

/// Human labels do not change the stored status names.
const fn human_task_status(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Ok => "Unchanged",
        TaskStatus::DryRun => "Planned",
        TaskStatus::Changed => "Changed",
        TaskStatus::Passed => "Passed",
        TaskStatus::NotApplicable => "Not applicable",
        TaskStatus::Skipped => "Skipped",
        TaskStatus::Blocked => "Blocked",
        TaskStatus::Interrupted => "Interrupted",
        TaskStatus::Failed => "Failed",
    }
}

pub(super) fn render_record(record: Record, verbose: bool) -> Option<(String, String)> {
    let rendered = match record {
        Record::RunStart {
            run_id,
            command,
            parent_run_id,
        } => (
            "run_start",
            format!(
                "{command} · {run_id}{}",
                parent_run_id.map_or_else(String::new, |id| format!(" · parent {id}"))
            ),
        ),
        Record::RunContext {
            profile,
            platform,
            dry_run,
        } => (
            "run_context",
            format!(
                "{profile} profile · {platform}{}",
                if dry_run { " · dry run" } else { "" }
            ),
        ),
        Record::RunScope {
            only,
            skip,
            with_deps,
            no_repo_update,
        } => (
            "run_scope",
            render_scope(&only, &skip, with_deps, no_repo_update),
        ),
        Record::RunFinish {
            outcome,
            elapsed_us,
            exit_code,
        } => (
            "run_finish",
            format!(
                "{} · {} · exit {exit_code}",
                outcome.label(),
                format_duration(elapsed_us)
            ),
        ),
        Record::TaskResult {
            name,
            status,
            reason,
            ..
        } => (
            "task_result",
            format!(
                "{name}: {}{}",
                human_task_status(status),
                reason.map_or_else(String::new, |reason| format!(": {reason}"))
            ),
        ),
        Record::TaskStart { name, .. } if verbose => ("task_start", name),
        Record::TaskDuration { elapsed_us, .. } if verbose => {
            ("task_timing", format_duration(elapsed_us))
        }
        Record::TaskStart { .. } | Record::TaskDuration { .. } => return None,
        Record::Action {
            message, planned, ..
        } => (
            if planned && !verbose {
                "plan"
            } else {
                "action"
            },
            message,
        ),
        Record::Command {
            command,
            outcome,
            exit_code,
            elapsed_us,
            stdout,
            stderr,
            stdout_bytes,
            stderr_bytes,
        } => {
            return render_command(
                &command,
                &outcome,
                exit_code,
                elapsed_us,
                [
                    ("stdout", stdout, stdout_bytes),
                    ("stderr", stderr, stderr_bytes),
                ],
                verbose,
            );
        }
        Record::Message { event, text } => {
            return (verbose || !diagnostic_event(&event)).then_some((event, text));
        }
    };
    Some((rendered.0.into(), rendered.1))
}

fn render_scope(only: &[String], skip: &[String], with_deps: bool, no_repo_update: bool) -> String {
    let mut scope = vec![selector_list(only, "all tasks")];
    if !skip.is_empty() {
        scope.push(format!("excluding {}", skip.join(", ")));
    }
    if with_deps {
        scope.push("including dependencies".into());
    }
    if no_repo_update {
        scope.push("repository sync disabled".into());
    }
    scope.join(" · ")
}

fn selector_list(selectors: &[String], empty: &str) -> String {
    if selectors.is_empty() {
        empty.into()
    } else {
        selectors.join(",")
    }
}

fn render_command(
    command: &str,
    outcome: &str,
    exit_code: Option<i32>,
    elapsed_us: u64,
    streams: [(&str, Option<String>, usize); 2],
    verbose: bool,
) -> Option<(String, String)> {
    if !verbose
        && matches!(outcome, "succeeded" | "nonzero_exit")
        && streams[1].1.as_deref().is_none_or(str::is_empty)
    {
        return None;
    }
    let label = if outcome == "nonzero_exit" {
        "nonzero exit (unchecked)"
    } else {
        outcome
    };
    let code = exit_code.map_or_else(|| "none".into(), |code| code.to_string());
    let mut lines = vec![if verbose {
        format!(
            "{command}: {label}, exit={code}, elapsed={}",
            format_duration(elapsed_us)
        )
    } else {
        format!(
            "{command}: {label} · exit {code} · {}",
            format_duration(elapsed_us)
        )
    }];
    for (stream, text, bytes) in streams {
        match text {
            Some(text) if !text.is_empty() => lines.push(format!("{stream}:\n{text}")),
            None if bytes > 0 => lines.push(format!("{stream}: {bytes} bytes omitted")),
            _ => {}
        }
    }
    Some(("command".into(), lines.join("\n")))
}

pub(super) fn format_duration(micros: u64) -> String {
    crate::infra::logging::format_elapsed(std::time::Duration::from_micros(micros))
}

pub(super) fn line_context(line: &str) -> Option<&str> {
    line.split_once(" [")?
        .1
        .split_once("] [")
        .map(|(context, _)| context)
}

/// Event delimiters are outside the context, which may itself contain brackets.
pub(super) fn line_event(line: &str) -> Option<&str> {
    line.split_once("] [")?
        .1
        .split_once(']')
        .map(|(event, _)| event)
}
