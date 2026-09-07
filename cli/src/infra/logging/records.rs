//! Versioned facts stored alongside human-readable diagnostic events.
use serde::{Deserialize, Serialize};

use super::{ActionCounts, TaskStatus};

/// Parent process identity passed only to a restarted or elevated invocation.
pub(crate) const PARENT_RUN_ARG: &str = "--parent-run-id";

/// Replace any inherited parent marker with this process's stable identity.
pub(crate) fn child_args(args: &[String], parent: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
            continue;
        }
        if arg == PARENT_RUN_ARG {
            skip = true;
            continue;
        }
        if arg.starts_with("--parent-run-id=") {
            continue;
        }
        out.push(arg.clone());
    }
    out.extend([PARENT_RUN_ARG.into(), parent.into()]);
    out
}

/// Completion of this process, distinct from individual task outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunOutcome {
    Succeeded,
    Failed,
    Interrupted,
}

impl RunOutcome {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

/// Machine-readable facts. Text remains text, including embedded newlines.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Record {
    RunStart {
        run_id: String,
        command: String,
        parent_run_id: Option<String>,
    },
    RunContext {
        profile: String,
        platform: String,
        dry_run: bool,
    },
    RunFinish {
        outcome: RunOutcome,
        elapsed_us: u64,
        exit_code: i32,
    },
    TaskResult {
        task_id: String,
        name: String,
        status: TaskStatus,
        reason: Option<String>,
        actions: ActionCounts,
    },
    TaskDuration {
        task_id: String,
        elapsed_us: u64,
    },
    Action {
        verb: String,
        subject: String,
        planned: bool,
        message: String,
    },
    Command {
        command: String,
        outcome: String,
        exit_code: Option<i32>,
        elapsed_us: u64,
        stdout: Option<String>,
        stderr: Option<String>,
        stdout_bytes: usize,
        stderr_bytes: usize,
    },
    Message {
        event: String,
        text: String,
    },
}

impl Record {
    fn clean(&mut self) {
        fn text(value: &mut String) {
            *value = super::utils::strip_ansi(value);
        }
        fn optional(value: &mut Option<String>) {
            if let Some(value) = value {
                text(value);
            }
        }
        match self {
            Self::RunStart {
                run_id,
                command,
                parent_run_id,
            } => {
                text(run_id);
                text(command);
                optional(parent_run_id);
            }
            Self::RunContext {
                profile, platform, ..
            } => {
                text(profile);
                text(platform);
            }
            Self::RunFinish { .. } => {}
            Self::TaskResult {
                task_id,
                name,
                reason,
                ..
            } => {
                text(task_id);
                text(name);
                optional(reason);
            }
            Self::TaskDuration { task_id, .. } => text(task_id),
            Self::Action {
                verb,
                subject,
                message,
                ..
            } => {
                text(verb);
                text(subject);
                text(message);
            }
            Self::Command {
                command,
                outcome,
                stdout,
                stderr,
                ..
            } => {
                text(command);
                text(outcome);
                optional(stdout);
                optional(stderr);
            }
            Self::Message {
                event,
                text: message,
            } => {
                text(event);
                text(message);
            }
        }
    }
}

/// Envelope schema can evolve without guessing fields from prose.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct StoredRecord {
    pub(crate) schema: u32,
    pub(crate) context: String,
    #[serde(flatten)]
    pub(crate) record: Record,
}

impl StoredRecord {
    pub(crate) fn new(context: &str, mut record: Record) -> Self {
        record.clean();
        Self {
            schema: 1,
            context: super::utils::strip_ansi(context),
            record,
        }
    }

    pub(crate) fn from_line(line: &str) -> Option<Self> {
        let (_, event) = line.split_once("] [")?;
        let message = event.strip_prefix("record] ")?;
        let record: Self = serde_json::from_str(message).ok()?;
        (record.schema == 1).then_some(record)
    }
}

pub(crate) fn elapsed_us(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

/// Send structured executor facts through the existing tracing subscriber.
pub(crate) fn trace_record(record: Record) {
    match serde_json::to_string(&StoredRecord::new(&super::log_thread_name(), record)) {
        Ok(json) => tracing::debug!(target: "dotfiles::record", "{json}"),
        Err(error) => super::runlog::warn_degraded(&format!("encoding record failed: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_linkage_replaces_ancestor_without_changing_scope() {
        for old in [
            vec!["--parent-run-id", "ancestor"],
            vec!["--parent-run-id=ancestor"],
        ] {
            let args: Vec<String> = [vec!["install", "--only", "symlinks", "--dry-run"], old]
                .concat()
                .into_iter()
                .map(str::to_string)
                .collect();
            assert_eq!(
                child_args(&args, "parent"),
                [
                    "install",
                    "--only",
                    "symlinks",
                    "--dry-run",
                    "--parent-run-id",
                    "parent"
                ]
            );
        }
    }
}
