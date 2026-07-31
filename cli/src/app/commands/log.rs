//! Log command implementation.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use crate::app::cli::LogOpts;
use crate::infra::logging::parse_run_log_file_name;

const NO_LOG_FOUND: &str = "No dotfiles log found yet.";
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
    if let Some(command) = opts.command.as_deref() {
        runs.retain(|run| run.command == command);
    }

    if runs.is_empty() {
        writeln!(out, "{NO_LOG_FOUND}").context("writing log output")?;
        return Ok(());
    }

    if opts.list {
        return write_run_list(&runs, out);
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
    write_log_contents(&contents, verbose, out)
}

/// Write a log file, hiding `[debug]` lines unless `verbose` is set.
///
/// This mirrors console behaviour, where debug messages are only rendered in
/// verbose mode. Header lines and anything that does not parse are always
/// written, so the filter can never silently swallow unexpected content.
fn write_log_contents(contents: &str, verbose: bool, out: &mut dyn std::io::Write) -> Result<()> {
    if verbose {
        return out
            .write_all(contents.as_bytes())
            .context("writing log output");
    }
    for line in contents.lines() {
        if line_event(line) == Some("debug") {
            continue;
        }
        writeln!(out, "{line}").context("writing log output")?;
    }
    Ok(())
}

/// Extract the event name from a run-log line.
///
/// Lines are `{seq} {elapsed} {wall} [{context}] [{event}] {message}`, so the
/// event is the second bracketed field.
fn line_event(line: &str) -> Option<&str> {
    let after_context_open = line.split_once('[')?.1;
    let after_context = after_context_open.split_once(']')?.1;
    let event = after_context.strip_prefix(" [")?;
    event.split_once(']').map(|(name, _)| name)
}

fn write_run_list(runs: &[RunEntry], out: &mut dyn std::io::Write) -> Result<()> {
    let command_width = runs
        .iter()
        .map(|run| run.command.len())
        .max()
        .unwrap_or(0)
        .max("COMMAND".len());
    writeln!(
        out,
        "  #  WHEN                  {:<command_width$}  SIZE",
        "COMMAND"
    )
    .context("writing log output")?;
    for (index, run) in runs.iter().enumerate() {
        writeln!(
            out,
            "{index:>3}  {:<20}  {:<command_width$}  {}",
            format_stamp(&run.stamp),
            run.command,
            format_size(run.size),
        )
        .context("writing log output")?;
    }
    Ok(())
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
        runs.push(RunEntry {
            stamp: parsed.stamp.to_string(),
            command: parsed.command.to_string(),
            path: entry.path(),
            size,
        });
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

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    fn opts() -> LogOpts {
        LogOpts {
            run: None,
            list: false,
            command: None,
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
    fn filters_by_command() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        write_run(dir, "20260731T154210Z-install-1.log", "install run\n");
        write_run(dir, "20260731T154902Z-update-2.log", "update run\n");

        let filtered = LogOpts {
            command: Some("install".to_string()),
            ..opts()
        };
        assert_eq!(capture(dir, &filtered, false), "install run\n");
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
}
