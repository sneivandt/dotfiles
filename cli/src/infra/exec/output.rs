//! Captured child-process diagnostics with explicit retention.
use super::{ExecError, ExecResult, OutputLog};
use crate::infra::logging::records::{Record, elapsed_us, trace_record};
use std::time::Duration;

pub(super) fn log_command_output(
    label: &str,
    result: &ExecResult,
    policy: OutputLog,
    elapsed: Duration,
) {
    log_result(
        label,
        result,
        policy,
        elapsed,
        if result.success {
            "succeeded"
        } else {
            "failed"
        },
    );
}

pub(super) fn log_command_error(
    label: &str,
    error: &ExecError,
    policy: OutputLog,
    elapsed: Duration,
) {
    match error {
        ExecError::Cancelled { result, .. } => {
            log_result(label, result, policy, elapsed, "interrupted");
        }
        ExecError::TimedOut { result, .. } => {
            log_result(label, result, policy, elapsed, "timed_out");
        }
        ExecError::NonZero { result, .. } => log_result(label, result, policy, elapsed, "failed"),
        ExecError::Spawn { .. } | ExecError::Io { .. } => {
            trace_record(Record::Command {
                command: label.into(),
                outcome: "failed".into(),
                exit_code: None,
                elapsed_us: elapsed_us(elapsed),
                stdout: None,
                stderr: Some(error.to_string()),
                stdout_bytes: 0,
                stderr_bytes: 0,
            });
        }
    }
}

fn log_result(
    label: &str,
    result: &ExecResult,
    policy: OutputLog,
    elapsed: Duration,
    outcome: &str,
) {
    let retain = policy != OutputLog::Omit;
    trace_record(Record::Command {
        command: label.into(),
        outcome: outcome.into(),
        exit_code: result.code,
        elapsed_us: elapsed_us(elapsed),
        stdout: (retain && (policy == OutputLog::Full || outcome != "succeeded"))
            .then(|| result.stdout.clone()),
        stderr: retain.then(|| result.stderr.clone()),
        stdout_bytes: result.stdout.len(),
        stderr_bytes: result.stderr.len(),
    });
}

/// Summarise a captured child-process output stream.
#[cfg(test)]
pub(super) fn stream_summary(output: &str) -> String {
    let line_count = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    if line_count == 0 {
        return String::new();
    }

    let line_word = if line_count == 1 { "line" } else { "lines" };
    format!("{line_count} {line_word}, {} bytes", output.len())
}

/// Format stdout/stderr for a failed command error message.
pub(super) fn failure_output(result: &ExecResult) -> String {
    let stdout = result.stdout.trim();
    let stderr = result.stderr.trim();
    format!(
        "stdout: {}; stderr: {}",
        if stdout.is_empty() { "<empty>" } else { stdout },
        if stderr.is_empty() { "<empty>" } else { stderr }
    )
}
