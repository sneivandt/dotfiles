use super::render::{
    LogSection, diagnostic_event, legacy_duplicate, line_context, line_event, record_section,
    render_record, text_record_parts, write_human_line, write_rendered_record,
};
use super::{
    BufRead, HashMap, HashSet, Record, RecordMetadata, Result, StoredRecord, StyleChoice, TextStyle,
};
use anyhow::Context as _;
use std::io::Seek;

fn write_section_break(
    out: &mut dyn std::io::Write,
    last_section: &mut Option<LogSection>,
    section: LogSection,
) -> Result<()> {
    if let Some(previous) = *last_section
        && previous != section
    {
        writeln!(out).context("writing log output")?;
    }
    *last_section = Some(section);
    Ok(())
}

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
    style: StyleChoice,
    out: &mut dyn std::io::Write,
) -> Result<()> {
    if raw && task.is_none() {
        std::io::copy(reader, out).context("writing raw log output")?;
        return Ok(());
    }
    let index = index_log(reader, task)?;
    reader.rewind().context("rewinding dotfiles log")?;
    let mut wrote_line = false;
    let mut last_section = None;
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
                if !verbose {
                    write_section_break(
                        out,
                        &mut last_section,
                        record_section(&stored.context, &event, &message),
                    )?;
                }
                write_rendered_record(
                    out,
                    &stored.context,
                    &index.task_names,
                    &event,
                    &message,
                    verbose.then_some(line.as_str()),
                    style,
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
                write_section_break(
                    out,
                    &mut last_section,
                    record_section(text_context, event, message),
                )?;
                write_rendered_record(
                    out,
                    text_context,
                    &index.task_names,
                    event,
                    message,
                    None,
                    style,
                )?;
            } else if !verbose && line.starts_with("# Dotfiles ") {
                write_section_break(out, &mut last_section, LogSection::Header)?;
                write_human_line(out, &style.paint(TextStyle::Dim, &line))?;
            } else {
                // Include unrecognized lines and record schemas in human views too.
                if !verbose {
                    write_section_break(out, &mut last_section, LogSection::Body)?;
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_selected_log_hides_run_context_after_startup_unless_verbose() {
        let startup = serde_json::to_string(&StoredRecord::new(
            "run",
            Record::Message {
                event: "startup".into(),
                text: "Update · desktop".into(),
            },
        ))
        .unwrap();
        let context = serde_json::to_string(&StoredRecord::new(
            "run",
            Record::RunContext {
                profile: "desktop".into(),
                platform: "linux".into(),
                dry_run: false,
            },
        ))
        .unwrap();
        let attempt = serde_json::to_string(&StoredRecord::new(
            "symlinks",
            Record::Message {
                event: "resource_apply".into(),
                text: "applying symlink".into(),
            },
        ))
        .unwrap();
        let result = serde_json::to_string(&StoredRecord::new(
            "symlinks",
            Record::TaskResult {
                task_id: "symlinks".into(),
                selector: Some("symlinks".into()),
                name: "Home symlinks".into(),
                status: crate::infra::logging::TaskStatus::Changed,
                reason: None,
                actions: crate::infra::logging::ActionCounts::default(),
            },
        ))
        .unwrap();
        let log_content = format!(
            "00:00:00.000 [run] [record] {startup}\n\
             00:00:00.001 [run] [record] {context}\n\
             00:00:00.002 [symlinks] [record] {attempt}\n\
             00:00:00.003 [symlinks] [record] {result}\n"
        );

        let mut concise = Vec::new();
        write_selected_log(
            &mut std::io::Cursor::new(log_content.as_bytes()),
            false,
            false,
            None,
            StyleChoice::plain(),
            &mut concise,
        )
        .unwrap();
        let concise_text = String::from_utf8(concise).unwrap();
        assert!(concise_text.contains("Update · desktop"), "{concise_text}");
        assert!(
            !concise_text.contains("platform=linux") && !concise_text.contains("desktop · linux"),
            "non-verbose view must skip duplicate RunContext when startup is present: {concise_text}"
        );
        assert!(
            !concise_text.contains("applying symlink"),
            "non-verbose view must suppress completed resource_apply attempts: {concise_text}"
        );

        let mut verbose = Vec::new();
        write_selected_log(
            &mut std::io::Cursor::new(log_content.as_bytes()),
            true,
            false,
            None,
            StyleChoice::plain(),
            &mut verbose,
        )
        .unwrap();
        let verbose_text = String::from_utf8(verbose).unwrap();
        assert!(verbose_text.contains("Update · desktop"), "{verbose_text}");
        assert!(
            verbose_text.contains("run_context") || verbose_text.contains("platform=linux"),
            "verbose view must keep RunContext even when startup is present: {verbose_text}"
        );
        assert!(
            verbose_text.contains("applying symlink"),
            "verbose view must retain completed resource_apply attempts: {verbose_text}"
        );
    }
}
