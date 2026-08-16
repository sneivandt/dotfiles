//! Captured command output formatting and run-log recording.

use super::ExecResult;

/// Log captured child-process output at debug level.
pub(super) fn log_command_output(label: &str, result: &ExecResult) {
    if !result.success {
        tracing::debug!(
            target: "dotfiles::exec",
            "command failed: {label}; exit status: {}",
            result.code.map_or_else(
                || "signal".to_string(),
                |code| code.to_string(),
            )
        );
        if result.stdout.trim().is_empty() {
            tracing::debug!(target: "dotfiles::exec", "{label} stdout: <empty>");
        }
        if result.stderr.trim().is_empty() {
            tracing::debug!(target: "dotfiles::exec", "{label} stderr: <empty>");
        }
    }
    log_stream(label, "stdout", &result.stdout, result.success);
    log_stream(label, "stderr", &result.stderr, result.success);
}

fn log_stream(label: &str, stream: &str, output: &str, success: bool) {
    let summary = stream_summary(output);
    if summary.is_empty() {
        return;
    }

    if success {
        tracing::debug!(
            target: "dotfiles::exec",
            "{label} {stream}: {summary} suppressed on success"
        );
        return;
    }

    tracing::debug!(target: "dotfiles::exec", "{label} {stream}: {summary}");
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        tracing::debug!(target: "dotfiles::exec", "{label} {stream}: {line}");
    }
}

/// Summarise a captured child-process output stream.
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
