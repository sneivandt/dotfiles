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
    loop {
        line.clear();
        let count = tail_reader
            .read_line(&mut line)
            .context("reading run metadata tail")?;
        if count == 0 {
            break;
        }
        tail_lines = tail_lines.saturating_add(1);
        parse_metadata_line(line.trim_end_matches(['\r', '\n']), run);
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
mod tests {
    use super::*;
    use crate::infra::logging::records::{Record, RunOutcome, StoredRecord};
    use std::io::Write as _;

    fn run_entry_for(path: std::path::PathBuf) -> RunEntry {
        let size = std::fs::metadata(&path).unwrap().len();
        RunEntry {
            stamp: "20260101T000000Z".into(),
            sort_stamp: "20260101T000000Z".into(),
            command: "update".into(),
            path,
            size,
            outcome: None,
            profile: None,
            elapsed_us: None,
            parent_run_id: None,
            started: false,
        }
    }

    fn write_record_line(file: &mut std::fs::File, record: Record) {
        let stored = StoredRecord::new("run", record);
        let json = serde_json::to_string(&stored).unwrap();
        writeln!(file, "00:00:00.000 [run] [record] {json}").unwrap();
    }

    #[test]
    fn read_run_metadata_reads_head_and_tail_on_large_logs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("20260101T000000Z-update.log");
        let mut file = std::fs::File::create(&path).unwrap();
        write_record_line(
            &mut file,
            Record::RunStart {
                run_id: "run-1".into(),
                command: "update".into(),
                parent_run_id: Some("parent-123".into()),
            },
        );
        write_record_line(
            &mut file,
            Record::RunContext {
                profile: "desktop".into(),
                platform: "linux".into(),
                dry_run: false,
            },
        );
        // Exceed 32 KiB with multi-byte UTF-8 payloads so the tail seek lands mid-line.
        let chunk = "é".repeat(120);
        for _ in 0..250 {
            writeln!(file, "00:00:01.000 [run] [debug] {chunk}").unwrap();
        }
        write_record_line(
            &mut file,
            Record::RunFinish {
                outcome: RunOutcome::Succeeded,
                elapsed_us: 987_654,
                exit_code: 0,
            },
        );
        drop(file);

        let mut entry = run_entry_for(path);
        assert!(
            entry.size > METADATA_HEAD_LIMIT + METADATA_TAIL_WINDOW,
            "fixture log must trigger the head/tail seek path (size={})",
            entry.size
        );
        read_run_metadata(&mut entry).unwrap();

        assert!(entry.started);
        assert_eq!(entry.parent_run_id.as_deref(), Some("parent-123"));
        assert_eq!(entry.profile.as_deref(), Some("desktop"));
        assert_eq!(entry.outcome, Some(RunOutcome::Succeeded));
        assert_eq!(entry.elapsed_us, Some(987_654));
    }

    #[test]
    fn read_run_metadata_handles_large_incomplete_logs_without_finish() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("20260101T000001Z-update.log");
        let mut file = std::fs::File::create(&path).unwrap();
        write_record_line(
            &mut file,
            Record::RunStart {
                run_id: "run-2".into(),
                command: "update".into(),
                parent_run_id: None,
            },
        );
        write_record_line(
            &mut file,
            Record::RunContext {
                profile: "base".into(),
                platform: "linux".into(),
                dry_run: false,
            },
        );
        let chunk = "x".repeat(256);
        for _ in 0..200 {
            writeln!(file, "00:00:01.000 [run] [debug] {chunk}").unwrap();
        }
        drop(file);

        let mut entry = run_entry_for(path);
        assert!(entry.size > METADATA_HEAD_LIMIT + METADATA_TAIL_WINDOW);
        read_run_metadata(&mut entry).unwrap();

        assert!(entry.started);
        assert_eq!(entry.profile.as_deref(), Some("base"));
        assert_eq!(entry.outcome, None);
        assert_eq!(entry.elapsed_us, None);
    }

    #[test]
    fn read_run_metadata_finds_run_context_beyond_head_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("20260101T000002Z-update.log");
        let mut file = std::fs::File::create(&path).unwrap();
        write_record_line(
            &mut file,
            Record::RunStart {
                run_id: "run-3".into(),
                command: "update".into(),
                parent_run_id: None,
            },
        );
        // Push `run_context` past `METADATA_HEAD_LIMIT` via a large `run_scope`.
        let only: Vec<String> = (0..1_200)
            .map(|i| format!("selector-name-{i:04}"))
            .collect();
        write_record_line(
            &mut file,
            Record::RunScope {
                only,
                skip: Vec::new(),
                with_deps: true,
                no_repo_update: false,
            },
        );
        write_record_line(
            &mut file,
            Record::RunContext {
                profile: "desktop".into(),
                platform: "linux".into(),
                dry_run: false,
            },
        );
        // Add > 32 KiB of intervening task lines so `run_context` is outside both
        // the initial head window and the tail window (total size > 50 KiB).
        for idx in 0..220 {
            write_record_line(
                &mut file,
                Record::TaskStart {
                    selector: format!("task-{idx:03}"),
                    name: format!("Task {idx:03} {}", "a".repeat(120)),
                },
            );
        }
        write_record_line(
            &mut file,
            Record::RunFinish {
                outcome: RunOutcome::Succeeded,
                elapsed_us: 123_456,
                exit_code: 0,
            },
        );
        drop(file);

        let mut entry = run_entry_for(path);
        assert!(
            entry.size > 50 * 1024,
            "fixture log must exceed 50 KiB (size={})",
            entry.size
        );
        read_run_metadata(&mut entry).unwrap();

        assert!(entry.started);
        assert_eq!(entry.profile.as_deref(), Some("desktop"));
        assert_eq!(entry.outcome, Some(RunOutcome::Succeeded));
        assert_eq!(entry.elapsed_us, Some(123_456));
    }

    #[test]
    fn read_run_metadata_full_scans_when_non_header_lines_precede_run_context() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("20260101T000003Z-update.log");
        let mut file = std::fs::File::create(&path).unwrap();
        write_record_line(
            &mut file,
            Record::RunStart {
                run_id: "run-4".into(),
                command: "update".into(),
                parent_run_id: None,
            },
        );
        // Push `run_context` past `METADATA_HEAD_LIMIT` with non-header records.
        for idx in 0..100 {
            write_record_line(
                &mut file,
                Record::Message {
                    event: "debug".into(),
                    text: format!("startup-note-{idx:03}-{}", "m".repeat(180)),
                },
            );
        }
        write_record_line(
            &mut file,
            Record::RunContext {
                profile: "workstation".into(),
                platform: "linux".into(),
                dry_run: false,
            },
        );
        for idx in 0..180 {
            write_record_line(
                &mut file,
                Record::TaskStart {
                    selector: format!("task-{idx:03}"),
                    name: format!("Task {idx:03} {}", "b".repeat(120)),
                },
            );
        }
        write_record_line(
            &mut file,
            Record::RunFinish {
                outcome: RunOutcome::Succeeded,
                elapsed_us: 654_321,
                exit_code: 0,
            },
        );
        drop(file);

        let mut entry = run_entry_for(path);
        assert!(entry.size > 50 * 1024);
        read_run_metadata(&mut entry).unwrap();

        assert!(entry.started);
        assert_eq!(entry.profile.as_deref(), Some("workstation"));
        assert_eq!(entry.outcome, Some(RunOutcome::Succeeded));
        assert_eq!(entry.elapsed_us, Some(654_321));
    }

    #[test]
    fn read_run_metadata_does_not_reread_large_legacy_text_log_after_eof() {
        use std::io::{Cursor, Read, Seek, SeekFrom};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

        struct CountingReader {
            inner: Cursor<Vec<u8>>,
            bytes_read: Arc<AtomicU64>,
            seeks: Arc<AtomicUsize>,
        }

        impl Read for CountingReader {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let count = self.inner.read(buf)?;
                self.bytes_read
                    .fetch_add(u64::try_from(count).unwrap_or(u64::MAX), Ordering::Relaxed);
                Ok(count)
            }
        }

        impl Seek for CountingReader {
            fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
                self.seeks.fetch_add(1, Ordering::Relaxed);
                self.inner.seek(pos)
            }
        }

        let mut payload = Vec::new();
        let line = format!(
            "00:00:01.000 [legacy] plain text entry {}\n",
            "x".repeat(200)
        );
        while u64::try_from(payload.len()).unwrap()
            <= METADATA_HEAD_LIMIT + METADATA_TAIL_WINDOW + 16 * 1024
        {
            payload.extend_from_slice(line.as_bytes());
        }
        let file_len = u64::try_from(payload.len()).unwrap();
        let bytes_read = Arc::new(AtomicU64::new(0));
        let seeks = Arc::new(AtomicUsize::new(0));
        let reader = CountingReader {
            inner: Cursor::new(payload),
            bytes_read: Arc::clone(&bytes_read),
            seeks: Arc::clone(&seeks),
        };
        let mut entry = RunEntry {
            stamp: "20260101T000004Z".into(),
            sort_stamp: "20260101T000004Z".into(),
            command: "update".into(),
            path: std::path::PathBuf::from("legacy.log"),
            size: file_len,
            outcome: None,
            profile: None,
            elapsed_us: None,
            parent_run_id: None,
            started: false,
        };

        read_run_metadata_from(reader, file_len, &mut entry).unwrap();

        assert!(!entry.started);
        assert_eq!(entry.profile, None);
        assert_eq!(entry.outcome, None);
        assert_eq!(entry.elapsed_us, None);
        assert_eq!(
            seeks.load(Ordering::Relaxed),
            0,
            "head scan reaching EOF must not seek to tail or rewind"
        );
        assert_eq!(
            bytes_read.load(Ordering::Relaxed),
            file_len,
            "legacy text log must be read exactly once"
        );
    }
}
