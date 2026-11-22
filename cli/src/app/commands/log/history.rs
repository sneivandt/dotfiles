use super::render::format_duration;
use super::{
    BufRead, DiscoveryFormat, Path, Record, RecordMetadata, Result, RunEntry, StoredRecord,
    parse_run_log_file_name, run_log_sort_stamp,
};
use anyhow::Context as _;
use serde::Serialize;

pub(super) fn write_run_list(
    runs: &[RunEntry],
    format: DiscoveryFormat,
    out: &mut dyn std::io::Write,
) -> Result<()> {
    match format {
        DiscoveryFormat::Table => write_run_table(runs, out),
        DiscoveryFormat::Plain => write_run_plain(runs, out),
        DiscoveryFormat::Json => write_run_json(runs, out),
    }
}

fn write_run_table(runs: &[RunEntry], out: &mut dyn std::io::Write) -> Result<()> {
    let command_width = runs
        .iter()
        .map(|run| run.command.len())
        .max()
        .unwrap_or(0)
        .max("COMMAND".len());
    let id_width = runs
        .iter()
        .map(|run| run.id().len())
        .max()
        .unwrap_or(0)
        .max("ID".len());
    writeln!(
        out,
        "  #  WHEN                  {:<id_width$}  {:<command_width$}  OUTCOME      DURATION  PROFILE  SIZE  PARENT",
        "ID", "COMMAND"
    )
    .context("writing log output")?;
    for (index, run) in runs.iter().enumerate() {
        writeln!(
            out,
            "{index:>3}  {:<20}  {:<id_width$}  {:<command_width$}  {:<11}  {:>8}  {}  {}  {}",
            format_stamp(&run.stamp),
            run.id(),
            run.command,
            run.outcome_label(),
            run.elapsed_us.map_or_else(|| "-".into(), format_duration),
            run.profile.as_deref().unwrap_or("-"),
            format_size(run.size),
            run.parent_run_id.as_deref().unwrap_or("-"),
        )
        .context("writing log output")?;
    }
    Ok(())
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

pub(super) fn read_run_metadata(run: &mut RunEntry) -> Result<()> {
    // Discovery reads headers; only history listing scans every run's records.
    let file = std::fs::File::open(&run.path)
        .with_context(|| format!("reading dotfiles log {}", run.path.display()))?;
    for line in std::io::BufReader::new(file).lines() {
        let line = line.context("reading run metadata")?;
        if let Some(metadata) = RecordMetadata::from_line(&line)
            && matches!(
                metadata.kind.as_str(),
                "run_start" | "run_context" | "run_finish"
            )
            && let Some(stored) = StoredRecord::from_line(&line)
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
    }
    Ok(())
}
