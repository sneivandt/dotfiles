use super::render::format_duration;
use super::{
    BufRead, DiscoveryFormat, Path, Record, RecordMetadata, Result, RunEntry, StoredRecord,
    StyleChoice, TextStyle, parse_run_log_file_name, run_log_sort_stamp,
};
use anyhow::Context as _;
use serde::Serialize;

pub(super) fn write_run_list(
    runs: &[RunEntry],
    format: DiscoveryFormat,
    style: StyleChoice,
    out: &mut dyn std::io::Write,
) -> Result<()> {
    match format {
        DiscoveryFormat::Table => write_run_table(runs, style, out),
        DiscoveryFormat::Plain => write_run_plain(runs, out),
        DiscoveryFormat::Json => write_run_json(runs, out),
    }
}

fn write_run_table(
    runs: &[RunEntry],
    style: StyleChoice,
    out: &mut dyn std::io::Write,
) -> Result<()> {
    let rows: Vec<_> = runs
        .iter()
        .enumerate()
        .map(|(index, run)| {
            (
                index.to_string(),
                format_stamp(&run.stamp),
                run.id(),
                run.command.as_str(),
                run.outcome_label(),
                run.elapsed_us.map_or_else(|| "-".into(), format_duration),
                run.profile.as_deref().unwrap_or("-"),
                format_size(run.size),
                run.parent_run_id.as_deref().unwrap_or("-"),
            )
        })
        .collect();
    let index_width = rows.iter().map(|row| row.0.len()).max().unwrap_or(0).max(3);
    let when_width = rows
        .iter()
        .map(|row| row.1.len())
        .max()
        .unwrap_or(0)
        .max(20);
    let id_width = rows
        .iter()
        .map(|row| row.2.len())
        .max()
        .unwrap_or(0)
        .max("ID".len());
    let command_width = rows
        .iter()
        .map(|row| row.3.len())
        .max()
        .unwrap_or(0)
        .max("Command".len());
    let outcome_width = rows
        .iter()
        .map(|row| row.4.len())
        .max()
        .unwrap_or(0)
        .max("Outcome".len());
    let duration_width = rows
        .iter()
        .map(|row| row.5.len())
        .max()
        .unwrap_or(0)
        .max("Duration".len());
    let profile_width = rows
        .iter()
        .map(|row| row.6.len())
        .max()
        .unwrap_or(0)
        .max("Profile".len());
    let size_width = rows
        .iter()
        .map(|row| row.7.len())
        .max()
        .unwrap_or(0)
        .max("Size".len());
    let header = format!(
        "{:>index_width$}  {:<when_width$}  {:<id_width$}  {:<command_width$}  {:<outcome_width$}  {:>duration_width$}  {:<profile_width$}  {:>size_width$}  Parent",
        "#", "When", "ID", "Command", "Outcome", "Duration", "Profile", "Size"
    );
    writeln!(out, "{}", style.paint(TextStyle::Dim, &header)).context("writing log output")?;
    for (index, when, id, command, outcome, duration, profile, size, parent) in rows {
        let outcome_tone = match outcome {
            "succeeded" => TextStyle::Green,
            "failed" => TextStyle::Red,
            "interrupted" => TextStyle::Yellow,
            _ => TextStyle::Dim,
        };
        let profile_cell = if profile == "-" {
            pad_left(profile, profile_width, style, Some(TextStyle::Dim))
        } else {
            pad_left(profile, profile_width, style, None)
        };
        writeln!(
            out,
            "{}  {}  {}  {}  {}  {}  {}  {}  {}",
            pad_right(&index, index_width, style, Some(TextStyle::Dim)),
            pad_left(&when, when_width, style, Some(TextStyle::Dim)),
            pad_left(&id, id_width, style, None),
            pad_left(command, command_width, style, Some(TextStyle::Cyan)),
            pad_left(outcome, outcome_width, style, Some(outcome_tone)),
            pad_right(&duration, duration_width, style, Some(TextStyle::Dim)),
            profile_cell,
            pad_right(&size, size_width, style, Some(TextStyle::Dim)),
            style.paint(TextStyle::Dim, parent),
        )
        .context("writing log output")?;
    }
    Ok(())
}

fn pad_left(text: &str, width: usize, style: StyleChoice, tone: Option<TextStyle>) -> String {
    let pad = width.saturating_sub(text.len());
    let rendered = tone.map_or_else(|| text.to_string(), |tone| style.paint(tone, text));
    format!("{rendered}{:pad$}", "")
}

fn pad_right(text: &str, width: usize, style: StyleChoice, tone: Option<TextStyle>) -> String {
    let pad = width.saturating_sub(text.len());
    let rendered = tone.map_or_else(|| text.to_string(), |tone| style.paint(tone, text));
    format!("{:pad$}{rendered}", "")
}

fn write_run_plain(runs: &[RunEntry], out: &mut dyn std::io::Write) -> Result<()> {
    for (index, run) in runs.iter().enumerate() {
        writeln!(
            out,
            "{index}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            format_stamp(&run.stamp),
            run.command,
            run.outcome_label(),
            run.elapsed_us.map_or_else(|| "-".into(), format_duration),
            run.profile.as_deref().unwrap_or("-"),
            run.size,
            run.id(),
            run.parent_run_id.as_deref().unwrap_or("-"),
        )
        .context("writing log output")?;
    }
    Ok(())
}

#[derive(Serialize)]
struct RunListing<'a> {
    index: usize,
    when: String,
    command: &'a str,
    outcome: &'static str,
    duration_us: Option<u64>,
    profile: Option<&'a str>,
    size_bytes: u64,
    id: String,
    parent_id: Option<&'a str>,
}

fn write_run_json(runs: &[RunEntry], out: &mut dyn std::io::Write) -> Result<()> {
    let listings = runs
        .iter()
        .enumerate()
        .map(|(index, run)| RunListing {
            index,
            when: format_stamp(&run.stamp),
            command: &run.command,
            outcome: run.outcome_label(),
            duration_us: run.elapsed_us,
            profile: run.profile.as_deref(),
            size_bytes: run.size,
            id: run.id(),
            parent_id: run.parent_run_id.as_deref(),
        })
        .collect::<Vec<_>>();
    serde_json::to_writer_pretty(&mut *out, &listings).context("writing log output")?;
    writeln!(out).context("writing log output")
}

/// Render `YYYYMMDDTHHMMSSZ` as `YYYY-MM-DD HH:MM:SSZ`.
///
/// Falls back to the raw stamp if it is not the expected shape, so listing
/// never fails on an unfamiliar file name.
fn format_stamp(stamp: &str) -> String {
    let Some(date) = stamp.get(0..8) else {
        return stamp.to_string();
    };
    let Some(time) = stamp.get(9..15) else {
        return stamp.to_string();
    };
    let part = |s: &str, range: std::ops::Range<usize>| s.get(range).unwrap_or("??").to_string();
    format!(
        "{}-{}-{} {}:{}:{}Z",
        part(date, 0..4),
        part(date, 4..6),
        part(date, 6..8),
        part(time, 0..2),
        part(time, 2..4),
        part(time, 4..6),
    )
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1_048_576;
    if bytes < KB {
        return format!("{bytes} B");
    }
    let (unit, scale) = if bytes < MB { ("KB", KB) } else { ("MB", MB) };
    let whole = bytes.checked_div(scale).unwrap_or(0);
    let tenths = bytes
        .checked_rem(scale)
        .and_then(|rem| rem.checked_mul(10))
        .and_then(|scaled| scaled.checked_div(scale))
        .unwrap_or(0);
    format!("{whole}.{tenths} {unit}")
}

/// Collect retained run logs, newest first.
///
/// Files that do not match the run-log naming pattern are ignored, so
/// unrelated files in the directory never appear in listings.
pub(super) fn discover_runs(log_dir: &Path) -> Result<Vec<RunEntry>> {
    let entries = match std::fs::read_dir(log_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("reading dotfiles log directory {}", log_dir.display()));
        }
    };
    let mut runs = Vec::new();

    for entry in entries {
        let entry =
            entry.with_context(|| format!("reading directory entry in {}", log_dir.display()))?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some(parsed) = parse_run_log_file_name(&name) else {
            continue;
        };
        let metadata = entry
            .metadata()
            .with_context(|| format!("reading dotfiles log metadata {}", entry.path().display()))?;
        if !metadata.is_file() {
            continue;
        }
        let run = RunEntry {
            stamp: parsed.stamp.to_string(),
            sort_stamp: run_log_sort_stamp(&entry.path(), parsed.stamp),
            command: parsed.command.to_string(),
            path: entry.path(),
            size: metadata.len(),
            outcome: None,
            profile: None,
            elapsed_us: None,
            parent_run_id: None,
            started: false,
        };
        runs.push(run);
    }

    // File names remain a deterministic fallback for indistinguishable starts.
    runs.sort_unstable_by(|a, b| {
        b.sort_stamp
            .cmp(&a.sort_stamp)
            .then_with(|| b.path.file_name().cmp(&a.path.file_name()))
    });
    Ok(runs)
}

const METADATA_HEAD_LIMIT: u64 = 16 * 1024;
const METADATA_TAIL_WINDOW: u64 = 16 * 1024;

pub(super) fn read_run_metadata(run: &mut RunEntry) -> Result<()> {
    // Discovery reads headers; only history listing inspects run records.
    let file = std::fs::File::open(&run.path)
        .with_context(|| format!("reading dotfiles log {}", run.path.display()))?;
    let file_len = file
        .metadata()
        .with_context(|| format!("reading dotfiles log metadata {}", run.path.display()))?
        .len();
    read_run_metadata_from(file, file_len, run)
}

fn read_run_metadata_from<R: std::io::Read + std::io::Seek>(
    mut file: R,
    file_len: u64,
    run: &mut RunEntry,
) -> Result<()> {
    use std::io::SeekFrom;

    if file_len <= METADATA_HEAD_LIMIT.saturating_add(METADATA_TAIL_WINDOW) {
        return scan_run_metadata_lines(std::io::BufReader::new(file), run);
    }

    let mut reader = std::io::BufReader::new(file);
    let mut bytes_read = 0_u64;
    let mut line = String::new();
    let mut saw_non_header = false;
    let mut reached_eof = false;
    while bytes_read < METADATA_HEAD_LIMIT || (!saw_non_header && run.profile.is_none()) {
        line.clear();
        let count = reader
            .read_line(&mut line)
            .context("reading run metadata")?;
        if count == 0 {
            reached_eof = true;
            break;
        }
        bytes_read = bytes_read.saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
        let kind = parse_metadata_line(line.trim_end_matches(['\r', '\n']), run);
        if let Some(kind) = kind
            && !matches!(kind.as_str(), "run_start" | "run_scope" | "run_context")
        {
            saw_non_header = true;
        }
        if run.started && run.profile.is_some() {
            break;
        }
    }

    // If the head scan already reached EOF, every line in the file was visited.
    if reached_eof {
        return Ok(());
    }

    file = reader.into_inner();
    let tail_start = file_len.saturating_sub(METADATA_TAIL_WINDOW);
    file.seek(SeekFrom::Start(tail_start))
        .context("seeking run log tail")?;
    let mut tail_reader = std::io::BufReader::new(file);
    // Skip the partial first line as raw bytes so an arbitrary seek offset
    // inside a multi-byte UTF-8 sequence cannot fail decoding.
    let mut discard = Vec::new();
    tail_reader
        .read_until(b'\n', &mut discard)
        .context("skipping partial tail line")?;
    let mut tail_lines = 0_usize;
    for tail_line in (&mut tail_reader).lines() {
        let tail_line = tail_line.context("reading run metadata tail")?;
        tail_lines = tail_lines.saturating_add(1);
        parse_metadata_line(&tail_line, run);
    }

    let needs_full_scan = !run.started
        || run.profile.is_none()
        || (run.outcome.is_none()
            && (tail_lines == 0
                || discard
                    .windows(b"\"run_finish\"".len())
                    .any(|window| window == b"\"run_finish\"")));
    if needs_full_scan {
        let mut rewound = tail_reader.into_inner();
        rewound
            .seek(SeekFrom::Start(0))
            .context("rewinding dotfiles log for full metadata scan")?;
        return scan_run_metadata_lines(std::io::BufReader::new(rewound), run);
    }
    Ok(())
}

fn scan_run_metadata_lines(reader: impl BufRead, run: &mut RunEntry) -> Result<()> {
    for line in reader.lines() {
        let line = line.context("reading run metadata")?;
        parse_metadata_line(&line, run);
    }
    Ok(())
}

fn parse_metadata_line(line: &str, run: &mut RunEntry) -> Option<String> {
    let metadata = RecordMetadata::from_line(line)?;
    if matches!(
        metadata.kind.as_str(),
        "run_start" | "run_context" | "run_finish"
    ) && let Some(stored) = StoredRecord::from_line(line)
    {
        match stored.record {
            Record::RunStart { parent_run_id, .. } => {
                run.started = true;
                run.parent_run_id = parent_run_id;
            }
            Record::RunContext { profile, .. } => run.profile = Some(profile),
            Record::RunFinish {
                outcome,
                elapsed_us,
                ..
            } => {
                run.outcome = Some(outcome);
                run.elapsed_us = Some(elapsed_us);
            }
            Record::RunScope { .. }
            | Record::TaskStart { .. }
            | Record::TaskResult { .. }
            | Record::TaskDuration { .. }
            | Record::Action { .. }
            | Record::Command { .. }
            | Record::Message { .. } => {}
        }
    }
    Some(metadata.kind)
}

#[cfg(test)]
#[path = "tests/history.rs"]
mod tests;
