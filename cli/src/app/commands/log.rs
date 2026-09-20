//! Log command implementation.

use crate::infra::logging::records::{Record, RunOutcome, StoredRecord};
use std::collections::HashSet;
use std::io::BufRead as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::Serialize;

use crate::app::cli::{DiscoveryFormat, LogOpts};
use crate::infra::logging::parse_run_log_file_name;

const NO_LOG_FOUND: &str = "No dotfiles log found yet.";
const NO_MATCHING_LOG: &str = "No retained runs match the requested filters.";
const LIST_HINT: &str = "Run 'dotfiles log --list' to see retained runs.";

/// A retained run log discovered in the log directory.
#[derive(Debug, PartialEq, Eq)]
struct RunEntry {
    /// Compact UTC start stamp, `YYYYMMDDTHHMMSSZ`.
    stamp: String,
    /// Command that produced the run.
    command: String,
    /// Path to the log file.
    path: PathBuf,
    /// Size of the log file in bytes.
    size: u64,
    outcome: Option<RunOutcome>,
    profile: Option<String>,
    elapsed_us: Option<u64>,
    parent_run_id: Option<String>,
    started: bool,
}

impl RunEntry {
    fn id(&self) -> String {
        self.path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }

    fn outcome_label(&self) -> &'static str {
        self.outcome.map_or(
            if self.started {
                "unfinished"
            } else {
                "unknown"
            },
            RunOutcome::label,
        )
    }
}

/// Run the log command.
///
/// # Errors
///
/// Returns an error if the log directory or selected log file cannot be read.
pub fn run(opts: &LogOpts, verbose: bool) -> Result<()> {
    let log_dir = crate::infra::logging::dotfiles_log_dir_readonly();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    run_in_dir(&log_dir, opts, verbose, &mut out)
}

fn run_in_dir(
    log_dir: &Path,
    opts: &LogOpts,
    verbose: bool,
    out: &mut dyn std::io::Write,
) -> Result<()> {
    let mut runs = discover_runs(log_dir)?;
    let logs_exist = !runs.is_empty();

    if let Some(id) = &opts.id {
        runs.retain(|run| run.id() == *id);
        if runs.is_empty() {
            anyhow::bail!("No retained run with ID {id}. {LIST_HINT}");
        }
    } else if let Some(command) = opts.command {
        runs.retain(|run| command.matches(&run.command));
    }
    if runs.is_empty() {
        let message = if logs_exist {
            NO_MATCHING_LOG
        } else {
            NO_LOG_FOUND
        };
        writeln!(out, "{message}").context("writing log output")?;
        return Ok(());
    }

    if opts.list {
        for run in &mut runs {
            read_run_metadata(run)?;
        }
        return write_run_list(&runs, opts.format.unwrap_or(DiscoveryFormat::Table), out);
    }

    let index = opts.run.unwrap_or(0);
    let Some(entry) = runs.get(index) else {
        anyhow::bail!(
            "No run at index {index} ({} retained). {LIST_HINT}",
            runs.len()
        );
    };

    let contents = std::fs::read_to_string(&entry.path)
        .with_context(|| format!("reading dotfiles log {}", entry.path.display()))?;
    write_selected_contents(&contents, verbose, opts.raw, opts.task.as_deref(), out)
}

fn write_selected_contents(
    contents: &str,
    verbose: bool,
    raw: bool,
    task: Option<&str>,
    out: &mut dyn std::io::Write,
) -> Result<()> {
    let selected_task_ids = task.map(|requested| {
        let mut identities = HashSet::from([requested.to_string()]);
        let normalized = crate::app::filter::normalize_task_filter(requested);
        for line in contents.lines() {
            let Some(stored) = StoredRecord::from_line(line) else {
                continue;
            };
            if let Record::TaskResult {
                task_id,
                selector: Some(selector),
                ..
            } = stored.record
                && crate::app::filter::normalize_task_filter(&selector) == normalized
            {
                identities.insert(task_id);
            }
        }
        identities
    });
    let mut wrote_line = false;
    for line in contents.lines() {
        let record = StoredRecord::from_line(line);
        let context = record
            .as_ref()
            .map(|r| r.context.as_str())
            .or_else(|| line_context(line));
        if selected_task_ids
            .as_ref()
            .is_some_and(|identities| context.is_none_or(|value| !identities.contains(value)))
        {
            continue;
        }
        if raw {
            writeln!(out, "{line}").context("writing log output")?;
            wrote_line = true;
            continue;
        }
        if let Some(stored) = record {
            if let Some((event, message)) = render_record(stored.record, verbose) {
                let prefix = line
                    .split_once(" [record] ")
                    .map_or("", |(prefix, _)| prefix);
                writeln!(out, "{prefix} [{event}] {message}").context("writing log output")?;
                wrote_line = true;
            }
        } else if verbose || line_event(line) != Some("debug") {
            // Old logs and unknown schemas remain readable rather than disappearing.
            writeln!(out, "{line}").context("writing log output")?;
            wrote_line = true;
        }
    }
    if task.is_some() && !wrote_line {
        anyhow::bail!("No records match the requested task selector or identity");
    }
    Ok(())
}

fn render_record(record: Record, verbose: bool) -> Option<(String, String)> {
    let rendered = match record {
        Record::RunStart {
            run_id,
            command,
            parent_run_id,
        } => (
            "run_start",
            format!(
                "{command} id={run_id}{}",
                parent_run_id.map_or_else(String::new, |id| format!(" parent={id}"))
            ),
        ),
        Record::RunContext {
            profile,
            platform,
            dry_run,
        } => (
            "run_context",
            format!("profile={profile} platform={platform} dry_run={dry_run}"),
        ),
        Record::RunFinish {
            outcome,
            elapsed_us,
            exit_code,
        } => (
            "run_finish",
            format!(
                "{} elapsed={} exit={exit_code}",
                outcome.label(),
                format_duration(elapsed_us)
            ),
        ),
        Record::TaskResult {
            task_id: _,
            selector: _,
            name,
            status,
            reason,
            actions: _,
        } => (
            "task_result",
            format!(
                "{name}: {status:?}{}",
                reason.map_or_else(String::new, |reason| format!(": {reason}"))
            ),
        ),
        Record::TaskDuration {
            task_id: _,
            elapsed_us,
        } => ("task_timing", format_duration(elapsed_us)),
        Record::Action { message, .. } => ("action", message),
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
            if !verbose && outcome == "succeeded" && stderr.as_deref().is_none_or(str::is_empty) {
                return None;
            }
            let mut lines = vec![format!(
                "{command}: {outcome}, exit={}, elapsed={}",
                exit_code.map_or_else(|| "none".into(), |code| code.to_string()),
                format_duration(elapsed_us)
            )];
            for (stream, text, bytes) in [
                ("stdout", stdout, stdout_bytes),
                ("stderr", stderr, stderr_bytes),
            ] {
                match text {
                    Some(text) if !text.is_empty() => {
                        lines.push(format!("{stream}:\n{text}"));
                    }
                    None if bytes > 0 => {
                        lines.push(format!("{stream}: {bytes} bytes omitted"));
                    }
                    _ => {}
                }
            }
            ("command", lines.join("\n"))
        }
        Record::Message { event, text } => {
            return (verbose || event != "debug").then_some((event, text));
        }
    };
    Some((rendered.0.into(), rendered.1))
}

fn format_duration(micros: u64) -> String {
    crate::infra::logging::format_elapsed(std::time::Duration::from_micros(micros))
}

fn line_context(line: &str) -> Option<&str> {
    line.split_once(" [")?
        .1
        .split_once("] [")
        .map(|(context, _)| context)
}

/// Event delimiters are outside the context, which may itself contain brackets.
fn line_event(line: &str) -> Option<&str> {
    line.split_once("] [")?
        .1
        .split_once(']')
        .map(|(event, _)| event)
}

fn write_run_list(
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
    writeln!(
        out,
        "  #  WHEN                  {:<command_width$}  OUTCOME      DURATION  PROFILE  SIZE  ID / PARENT",
        "COMMAND"
    )
    .context("writing log output")?;
    for (index, run) in runs.iter().enumerate() {
        writeln!(
            out,
            "{index:>3}  {:<20}  {:<command_width$}  {:<11}  {:>8}  {}  {}  {}{}",
            format_stamp(&run.stamp),
            run.command,
            run.outcome_label(),
            run.elapsed_us.map_or_else(|| "-".into(), format_duration),
            run.profile.as_deref().unwrap_or("-"),
            format_size(run.size),
            run.id(),
            run.parent_run_id
                .as_ref()
                .map_or_else(String::new, |parent| format!(" / {parent}")),
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
fn discover_runs(log_dir: &Path) -> Result<Vec<RunEntry>> {
    if !log_dir.is_dir() {
        return Ok(Vec::new());
    }

    let entries = std::fs::read_dir(log_dir)
        .with_context(|| format!("reading dotfiles log directory {}", log_dir.display()))?;
    let mut runs = Vec::new();

    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some(parsed) = parse_run_log_file_name(&name) else {
            continue;
        };
        let size = entry.metadata().map_or(0, |meta| meta.len());
        let run = RunEntry {
            stamp: parsed.stamp.to_string(),
            command: parsed.command.to_string(),
            path: entry.path(),
            size,
            outcome: None,
            profile: None,
            elapsed_us: None,
            parent_run_id: None,
            started: false,
        };
        runs.push(run);
    }

    // Stamps are fixed width, so descending lexical order is newest first.
    // The file name breaks ties between runs that started in the same second.
    runs.sort_unstable_by(|a, b| {
        b.stamp
            .cmp(&a.stamp)
            .then_with(|| b.path.file_name().cmp(&a.path.file_name()))
    });
    Ok(runs)
}

fn read_run_metadata(run: &mut RunEntry) -> Result<()> {
    // Only history listing scans metadata; selecting one run reads only that file.
    let file = std::fs::File::open(&run.path)
        .with_context(|| format!("reading dotfiles log {}", run.path.display()))?;
    for line in std::io::BufReader::new(file).lines() {
        let line = line.context("reading run metadata")?;
        if let Some(record) = StoredRecord::from_line(&line) {
            match record.record {
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
                Record::TaskResult { .. }
                | Record::TaskDuration { .. }
                | Record::Action { .. }
                | Record::Command { .. }
                | Record::Message { .. } => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> LogOpts {
        LogOpts {
            run: None,
            id: None,
            task: None,
            raw: false,
            list: false,
            format: None,
            command: None,
            verbose: false,
        }
    }

    fn write_run(dir: &Path, name: &str, contents: &str) {
        std::fs::create_dir_all(dir).expect("create log dir");
        std::fs::write(dir.join(name), contents).expect("write log");
    }

    fn capture(dir: &Path, opts: &LogOpts, verbose: bool) -> String {
        let mut output = Vec::new();
        run_in_dir(dir, opts, verbose, &mut output).expect("log command should succeed");
        String::from_utf8(output).unwrap()
    }

    #[test]
    fn prints_latest_run_by_default() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        write_run(dir, "20260731T154210Z-install-1.log", "older\n");
        write_run(dir, "20260731T154902Z-update-2.log", "newer\n");

        assert_eq!(capture(dir, &opts(), false), "newer\n");
    }

    #[test]
    fn selects_run_by_index() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        write_run(dir, "20260731T154210Z-install-1.log", "older\n");
        write_run(dir, "20260731T154902Z-update-2.log", "newer\n");

        let selected = LogOpts {
            run: Some(1),
            ..opts()
        };
        assert_eq!(capture(dir, &selected, false), "older\n");
    }

    #[test]
    fn rejects_out_of_range_index() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        write_run(dir, "20260731T154210Z-install-1.log", "only\n");

        let selected = LogOpts {
            run: Some(7),
            ..opts()
        };
        let mut output = Vec::new();
        let err = run_in_dir(dir, &selected, false, &mut output)
            .expect_err("out of range index should fail");
        let message = err.to_string();
        assert!(message.contains("No run at index 7"), "{message}");
        assert!(message.contains("1 retained"), "{message}");
        assert!(message.contains("--list"), "{message}");
    }

    #[test]
    fn install_and_update_have_separate_command_filters() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        write_run(dir, "20260731T154210Z-install-1.log", "install run\n");
        write_run(dir, "20260731T154902Z-update-2.log", "update run\n");

        let install = LogOpts {
            command: Some(crate::app::cli::LogCommand::Install),
            ..opts()
        };
        assert_eq!(capture(dir, &install, false), "install run\n");
        let update = LogOpts {
            command: Some(crate::app::cli::LogCommand::Update),
            ..opts()
        };
        assert_eq!(capture(dir, &update, false), "update run\n");
    }

    #[test]
    fn canonical_command_filters_include_legacy_aliases() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        write_run(dir, "20260731T154210Z-check-1.log", "check run\n");
        write_run(dir, "20260731T154902Z-test-2.log", "test run\n");

        let filtered = LogOpts {
            command: Some(crate::app::cli::LogCommand::Check),
            ..opts()
        };
        assert_eq!(capture(dir, &filtered, false), "test run\n");

        let listed = LogOpts {
            list: true,
            ..filtered
        };
        let output = capture(dir, &listed, false);
        assert!(output.contains("check"), "{output}");
        assert!(output.contains("test"), "{output}");
    }

    #[test]
    fn lists_runs_newest_first() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        write_run(dir, "20260731T154210Z-install-1.log", "a");
        write_run(dir, "20260731T154902Z-update-2.log", "bb");

        let listed = LogOpts {
            list: true,
            ..opts()
        };
        let output = capture(dir, &listed, false);
        let lines: Vec<&str> = output.lines().collect();
        assert!(lines[0].contains("WHEN"), "{output}");
        assert!(
            lines[1].starts_with("  0  2026-07-31 15:49:02Z  update"),
            "{output}"
        );
        assert!(
            lines[2].starts_with("  1  2026-07-31 15:42:10Z  install"),
            "{output}"
        );
    }

    #[test]
    fn lists_runs_as_plain_text_and_json() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        write_run(dir, "20260731T154210Z-install-1.log", "a");

        let plain = capture(
            dir,
            &LogOpts {
                list: true,
                format: Some(DiscoveryFormat::Plain),
                ..opts()
            },
            false,
        );
        assert!(
            plain.starts_with("0\t2026-07-31 15:42:10Z\tinstall\t"),
            "{plain}"
        );

        let json = capture(
            dir,
            &LogOpts {
                list: true,
                format: Some(DiscoveryFormat::Json),
                ..opts()
            },
            false,
        );
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(value[0]["command"], "install");
        assert_eq!(value[0]["size_bytes"], 1);
    }

    #[test]
    fn distinguishes_absent_logs_from_an_empty_filter_result() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_run(tmp.path(), "20260731T154210Z-install-1.log", "a");
        let output = capture(
            tmp.path(),
            &LogOpts {
                command: Some(crate::app::cli::LogCommand::Check),
                ..opts()
            },
            false,
        );
        assert_eq!(output, "No retained runs match the requested filters.\n");
    }

    #[test]
    fn hides_debug_lines_unless_verbose() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        let contents = concat!(
            "# Dotfiles dev-0.0.0\n",
            "000001 +          10 2026-07-31T15:42:10.000000Z [main] [info] visible\n",
            "000002 +          20 2026-07-31T15:42:10.000000Z [main] [debug] hidden\n",
        );
        write_run(dir, "20260731T154210Z-install-1.log", contents);

        let quiet = capture(dir, &opts(), false);
        assert!(quiet.contains("visible"), "{quiet}");
        assert!(!quiet.contains("hidden"), "{quiet}");
        assert!(quiet.contains("# Dotfiles"), "{quiet}");

        let loud = capture(dir, &opts(), true);
        assert_eq!(loud, contents);
    }

    #[test]
    fn ignores_unrelated_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        write_run(dir, "notes.txt", "not a run log\n");
        write_run(dir, "install.log", "legacy name\n");

        assert_eq!(capture(dir, &opts(), false), "No dotfiles log found yet.\n");
    }

    #[test]
    fn prints_missing_log_message() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("absent");

        assert_eq!(
            capture(&dir, &opts(), false),
            "No dotfiles log found yet.\n"
        );
    }
    fn record(context: &str, record: Record) -> String {
        format!(
            "000001 +          10 2026-09-07T10:00:00.000000Z [{context}] [record] {}\n",
            serde_json::to_string(&StoredRecord::new(context, record)).unwrap()
        )
    }

    #[test]
    fn history_and_stable_selection_survive_newer_runs_and_missing_finish() {
        let tmp = tempfile::tempdir().unwrap();
        let id = "20260907T100000Z-install-1";
        let content = record(
            "main",
            Record::RunStart {
                run_id: id.into(),
                command: "install".into(),
                parent_run_id: Some("parent-id".into()),
            },
        ) + &record(
            "main",
            Record::RunContext {
                profile: "desktop".into(),
                platform: "Linux".into(),
                dry_run: true,
            },
        ) + &record(
            "main",
            Record::RunFinish {
                outcome: RunOutcome::Failed,
                elapsed_us: 1_200_000,
                exit_code: 1,
            },
        );
        write_run(tmp.path(), &format!("{id}.log"), &content);
        write_run(
            tmp.path(),
            "20260907T100001Z-install-2.log",
            &record(
                "main",
                Record::RunStart {
                    run_id: "newer".into(),
                    command: "install".into(),
                    parent_run_id: None,
                },
            ),
        );
        write_run(tmp.path(), "20260906T100000Z-check-3.log", "legacy log\n");
        let listing = capture(
            tmp.path(),
            &LogOpts {
                list: true,
                ..opts()
            },
            false,
        );
        for expected in [
            "failed",
            "unfinished",
            "unknown",
            "desktop",
            "1.2s",
            id,
            "parent-id",
        ] {
            assert!(listing.contains(expected), "missing {expected}: {listing}");
        }
        let selected = capture(
            tmp.path(),
            &LogOpts {
                id: Some(id.into()),
                raw: true,
                ..opts()
            },
            false,
        );
        assert_eq!(selected, content);
        let invalid = run_in_dir(
            tmp.path(),
            &LogOpts {
                id: Some("../not-a-log".into()),
                ..opts()
            },
            false,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(invalid.to_string().contains("No retained run"));
    }

    #[test]
    fn failed_command_and_successful_stderr_are_visible_without_verbose() {
        let command = |outcome: &str, stderr: &str| Record::Command {
            command: "example".into(),
            outcome: outcome.into(),
            exit_code: Some(i32::from(outcome == "failed")),
            elapsed_us: 10,
            stdout: Some("first\n  second\n\nlast".into()),
            stderr: Some(stderr.into()),
            stdout_bytes: 20,
            stderr_bytes: stderr.len(),
        };
        let content = record("task-a", command("failed", "cause"))
            + &record("task-b", command("succeeded", "warning"));
        let mut out = Vec::new();
        write_selected_contents(&content, false, false, None, &mut out).unwrap();
        let output = String::from_utf8(out).unwrap();
        for expected in ["first\n  second\n\nlast", "cause", "warning", "exit=1"] {
            assert!(output.contains(expected), "{output}");
        }
        let mut selected = Vec::new();
        write_selected_contents(&content, false, false, Some("task-a"), &mut selected).unwrap();
        let selected = String::from_utf8(selected).unwrap();
        assert!(selected.contains("cause"));
        assert!(!selected.contains("warning"));
    }

    #[test]
    fn task_selector_resolves_to_the_stored_stable_identity() {
        let task_id = "dotfiles_cli::domains::files::symlinks::InstallSymlinks";
        let content = record(
            task_id,
            Record::Message {
                event: "info".into(),
                text: "selected detail".into(),
            },
        ) + &record(
            task_id,
            Record::TaskResult {
                task_id: task_id.into(),
                selector: Some("symlinks".into()),
                name: "Home symlinks".into(),
                status: crate::infra::logging::TaskStatus::Changed,
                reason: None,
                actions: crate::infra::logging::ActionCounts::default(),
            },
        ) + &record(
            "other-task",
            Record::Message {
                event: "info".into(),
                text: "other detail".into(),
            },
        );
        let mut selected = Vec::new();
        write_selected_contents(&content, false, false, Some("symlinks"), &mut selected).unwrap();
        let selected = String::from_utf8(selected).unwrap();
        assert!(selected.contains("selected detail"), "{selected}");
        assert!(selected.contains("Home symlinks"), "{selected}");
        assert!(!selected.contains("other detail"), "{selected}");
    }

    #[test]
    fn unknown_schema_and_malformed_records_remain_visible() {
        let content = "000001 +10 now [task] [record] {\"schema\":99}\n000002 +20 now [task] [record] {broken\n";
        let mut output = Vec::new();
        write_selected_contents(content, false, false, None, &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), content);
        assert!(
            StoredRecord::from_line("000001 +10 now [task] [info] quoted [record] {\"schema\":1}")
                .is_none()
        );
    }
}
