//! Log command implementation.

mod history;
mod render;
mod selection;

use history::{discover_runs, read_run_metadata, write_run_list};
#[cfg(test)]
use render::render_record;
use selection::write_selected_log;

use crate::infra::logging::records::{Record, RunOutcome, StoredRecord};
use std::collections::{HashMap, HashSet};
use std::io::BufRead;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::Deserialize;

use crate::app::cli::{DiscoveryFormat, LogOpts};
use crate::infra::logging::{TaskStatus, parse_run_log_file_name, run_log_sort_stamp};

const NO_LOG_FOUND: &str = "No dotfiles log found yet.";
const NO_MATCHING_LOG: &str = "No retained runs match the requested filters.";
const LIST_HINT: &str = "Run 'dotfiles log --list' to see retained runs.";

/// A retained run log discovered in the log directory.
#[derive(Debug, PartialEq, Eq)]
struct RunEntry {
    /// Compact UTC start stamp, `YYYYMMDDTHHMMSSZ`.
    stamp: String,
    /// Start timestamp with subsecond precision when available in the header.
    sort_stamp: String,
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
    if runs.is_empty() && opts.run.is_none() {
        if opts.list
            && let Some(format @ (DiscoveryFormat::Json | DiscoveryFormat::Plain)) = opts.format
        {
            return write_run_list(&runs, format, out);
        }
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

    let file = std::fs::File::open(&entry.path)
        .with_context(|| format!("reading dotfiles log {}", entry.path.display()))?;
    write_selected_log(
        &mut std::io::BufReader::new(file),
        verbose,
        opts.raw,
        opts.task.as_deref(),
        out,
    )
    .with_context(|| format!("reading dotfiles log {}", entry.path.display()))
}

#[cfg(test)]
fn write_selected_contents(
    contents: &str,
    verbose: bool,
    raw: bool,
    task: Option<&str>,
    out: &mut dyn std::io::Write,
) -> Result<()> {
    write_selected_log(
        &mut std::io::Cursor::new(contents.as_bytes()),
        verbose,
        raw,
        task,
        out,
    )
}

// Unlike StoredRecord, this projection skips command/message payloads without
// allocating their strings. It serves both selection indexing and run history.
#[derive(Deserialize)]
struct RecordMetadata {
    schema: u32,
    context: String,
    #[serde(rename = "type")]
    kind: String,
}

impl RecordMetadata {
    fn from_line(line: &str) -> Option<Self> {
        let (_, event) = line.split_once("] [")?;
        let message = event.strip_prefix("record] ")?;
        let metadata: Self = serde_json::from_str(message).ok()?;
        (metadata.schema == 1).then_some(metadata)
    }
}

#[cfg(test)]
mod tests;
