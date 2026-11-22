use super::render::{
    diagnostic_event, legacy_duplicate, line_context, line_event, render_record, text_record_parts,
    write_human_line, write_rendered_record,
};
use super::{BufRead, HashMap, HashSet, Record, RecordMetadata, Result, StoredRecord};
use anyhow::Context as _;
use std::io::Seek;

#[derive(Default)]
struct LogIndex {
    has_startup: bool,
    selected_task_ids: Option<HashSet<String>>,
    results: HashSet<String>,
    durations: HashSet<String>,
    task_names: HashMap<String, String>,
}

impl LogIndex {
    fn completed_resource_attempt(&self, context: &str, event: &str) -> bool {
        matches!(event, "resource_apply" | "resource_remove") && self.results.contains(context)
    }
}

fn index_log(reader: &mut impl BufRead, task: Option<&str>) -> Result<LogIndex> {
    let mut index = LogIndex {
        selected_task_ids: task.map(|requested| HashSet::from([requested.to_string()])),
        ..LogIndex::default()
    };
    let normalized = task.map(crate::app::filter::normalize_task_filter);
    // Older runs wrote text completion/timing events alongside structured facts.
    // Prefer the structured facts without altering raw or unknown-schema output.
    for line in reader.lines() {
        let line = line.context("reading log index")?;
        if let Some((_, "startup", _)) = text_record_parts(&line) {
            index.has_startup = true;
        }
        let Some(metadata) = RecordMetadata::from_line(&line) else {
            continue;
        };
        if !matches!(
            metadata.kind.as_str(),
            "task_start" | "task_result" | "task_duration" | "message"
        ) {
            continue;
        }
        let Some(stored) = StoredRecord::from_line(&line) else {
            continue;
        };
        match stored.record {
            Record::TaskStart { selector, name } => {
                if let (Some(identities), Some(normalized)) =
                    (&mut index.selected_task_ids, &normalized)
                    && crate::app::filter::normalize_task_filter(&selector) == *normalized
                {
                    identities.insert(stored.context.clone());
                }
                index.task_names.insert(stored.context, name);
            }
            Record::TaskResult {
                task_id,
                selector,
                name,
                ..
            } => {
                if let (Some(identities), Some(normalized), Some(selector)) =
                    (&mut index.selected_task_ids, &normalized, selector)
                    && crate::app::filter::normalize_task_filter(&selector) == *normalized
                {
                    identities.insert(task_id.clone());
                }
                index.task_names.insert(task_id, name);
                index.results.insert(stored.context);
            }
            Record::TaskDuration { .. } => {
                index.durations.insert(stored.context);
            }
            Record::Message { event, text } => {
                if event == "task_start" {
                    index.task_names.entry(stored.context).or_insert(text);
                } else if event == "startup" {
                    index.has_startup = true;
                }
            }
            Record::RunStart { .. }
            | Record::RunContext { .. }
            | Record::RunScope { .. }
            | Record::RunFinish { .. }
            | Record::Action { .. }
            | Record::Command { .. } => {}
        }
    }
    Ok(index)
}

pub(super) fn write_selected_log(
    reader: &mut (impl BufRead + Seek),
    verbose: bool,
    raw: bool,
    task: Option<&str>,
    out: &mut dyn std::io::Write,
) -> Result<()> {
    if raw && task.is_none() {
        std::io::copy(reader, out).context("writing raw log output")?;
        return Ok(());
    }
    let index = index_log(reader, task)?;
    reader.rewind().context("rewinding dotfiles log")?;
    let mut wrote_line = false;
    for line in reader.lines() {
        let line = line.context("reading dotfiles log")?;
        if !raw && !verbose && line.starts_with("# Columns:") {
            continue;
        }
        if let Some(identities) = &index.selected_task_ids {
            let metadata = RecordMetadata::from_line(&line);
            let context = metadata
                .as_ref()
                .map(|record| record.context.as_str())
                .or_else(|| line_context(&line));
            if context.is_none_or(|value| !identities.contains(value)) {
                continue;
            }
        }
        if raw {
            writeln!(out, "{line}").context("writing log output")?;
            wrote_line = true;
            continue;
        }
        let record = StoredRecord::from_line(&line);
        let context = record
            .as_ref()
            .map(|r| r.context.as_str())
            .or_else(|| line_context(&line));
        if let Some(stored) = record {
            // The startup header also identifies WSL and the selected overlay.
            if !verbose && index.has_startup && matches!(stored.record, Record::RunContext { .. }) {
                continue;
            }
            if let Some((event, message)) = render_record(stored.record, verbose) {
                if !verbose && index.completed_resource_attempt(&stored.context, &event) {
                    continue;
                }
                write_rendered_record(
                    out,
                    &stored.context,
                    &index.task_names,
                    &event,
                    &message,
                    verbose.then_some(line.as_str()),
                )?;
                wrote_line = true;
            }
        } else if !legacy_duplicate(&line, context, &index.results, &index.durations)
            && (verbose || line_event(&line).is_none_or(|event| !diagnostic_event(event)))
        {
            if !verbose && let Some((text_context, event, message)) = text_record_parts(&line) {
                if index.completed_resource_attempt(text_context, event) {
                    continue;
                }
                write_rendered_record(out, text_context, &index.task_names, event, message, None)?;
            } else {
                // Include unrecognized lines and record schemas in human views too.
                write_human_line(out, &line)?;
            }
            wrote_line = true;
        }
    }
    if task.is_some() && !wrote_line {
        anyhow::bail!("No records match the requested task selector or identity");
    }
    Ok(())
}
