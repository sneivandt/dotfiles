use super::*;

#[test]
fn command_diagnostic_label_includes_safe_arguments_and_working_directory() {
    let spec = CommandSpec::new("systemctl")
        .args(&["--user", "daemon-reload"])
        .current_dir("/tmp/work tree");

    assert_eq!(
        spec.label(),
        "systemctl --user daemon-reload (in /tmp/work tree)"
    );
}

#[test]
fn command_diagnostic_label_can_redact_sensitive_arguments() {
    let spec = CommandSpec::new("credential-helper")
        .args(&["--token", "secret-value"])
        .redact_arguments();

    assert_eq!(spec.label(), "credential-helper [arguments redacted]");
    assert!(
        !spec.label().contains("secret-value"),
        "redacted diagnostics must not expose argument values"
    );
}

#[test]
fn nonzero_error_reports_command_status_stdout_and_stderr() {
    let error = ExecError::non_zero(
        "systemctl --user daemon-reload",
        ExecResult::failure("out", "Failed to connect to bus", Some(1)),
    );
    assert_eq!(
        error.to_string(),
        "systemctl --user daemon-reload failed (exit 1): stdout: out; stderr: Failed to connect to bus"
    );
}

#[test]
fn typed_errors_preserve_display_and_io_sources() {
    for (case, error, expected, source) in [
        (
            "cancelled_empty_streams",
            ExecError::Cancelled {
                command: "tool".into(),
                result: ExecResult::failure(" \n", "", None),
            },
            "tool cancelled: stdout: <empty>; stderr: <empty>",
            None,
        ),
        (
            "timeout_whole_seconds_and_trimmed_multiline_output",
            ExecError::TimedOut {
                command: "tool".into(),
                timeout: Duration::from_millis(1500),
                result: ExecResult::failure(" out\nnext\n", " err ", None),
            },
            "tool timed out after 1 seconds: stdout: out\nnext; stderr: err",
            None,
        ),
        (
            "nonzero_without_exit_code",
            ExecError::non_zero("tool", ExecResult::failure("", "", None)),
            "tool failed (exit -1): stdout: <empty>; stderr: <empty>",
            None,
        ),
        (
            "spawn_error",
            ExecError::spawn("tool", io::Error::other("spawn failure")),
            "failed to execute tool: spawn failure",
            Some("spawn failure"),
        ),
        (
            "io_error",
            ExecError::Io {
                command: "tool".into(),
                operation: "reading stdout",
                source: io::Error::other("read failure"),
            },
            "reading stdout for tool: read failure",
            Some("read failure"),
        ),
    ] {
        assert_eq!(error.to_string(), expected, "{case}");
        let actual_source = std::error::Error::source(&error);
        assert_eq!(
            actual_source.map(ToString::to_string).as_deref(),
            source,
            "{case}"
        );
        assert_eq!(
            error.io_error().map(ToString::to_string).as_deref(),
            source,
            "{case}"
        );
        if let Some(actual_source) = actual_source {
            assert!(
                actual_source.downcast_ref::<io::Error>().is_some(),
                "{case}"
            );
        }
    }
}

#[test]
fn missing_program_returns_typed_spawn_error() {
    let executor = ProcessExecutor::system();
    let result = executor.execute(CommandSpec::new(
        "dotfiles-this-program-does-not-exist-12345",
    ));

    assert!(
        matches!(result, Err(ExecError::Spawn { .. })),
        "missing executable should produce a typed spawn error"
    );
}

#[test]
fn invalid_working_directory_preserves_spawn_source_and_context() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing");
    #[cfg(windows)]
    let program = "cmd";
    #[cfg(not(windows))]
    let program = "sh";
    let spec = CommandSpec::new(program).current_dir(&missing);
    let label = spec.label();
    let error = ProcessExecutor::system().execute(spec).unwrap_err();
    let ExecError::Spawn { command, source } = error else {
        panic!("invalid cwd must fail before executing: {error}");
    };
    assert_eq!(command, label);
    #[cfg(windows)]
    assert_eq!(source.kind(), io::ErrorKind::NotADirectory);
    #[cfg(not(windows))]
    assert_eq!(source.kind(), io::ErrorKind::NotFound);
    assert!(!missing.exists());
}

#[test]
fn command_failure_summary_keeps_one_cause_and_omits_stream_dump() {
    let error = ExecError::non_zero(
        "example",
        ExecResult::failure("large stdout", "first cause\nsecond detail", Some(2)),
    );
    assert_eq!(
        error.concise_message(),
        "example failed (exit 2): first cause"
    );
    assert!(error.to_string().contains("second detail"));
}

#[test]
fn command_failure_summary_prefers_diagnostics_over_notices() {
    let cases = [
        (
            "APM wrapped clone failure",
            "[!] A new version of APM is available\n\
             [>] Checking upstream for revision-pin freshness...\n\
             [x] 2 packages failed:\n\
             example/skills/tool -- Failed to install dependency: Failed to clone\n\
             repository: [Errno 2] No such file or directory: '/usr/bin/git'\n\
             One or more direct dependencies failed validation.",
            "",
            "repository: [Errno 2] No such file or directory: '/usr/bin/git'",
        ),
        (
            "stderr notice before fatal error",
            "progress",
            "warning: update available\n fatal: unable to access remote\nmore detail",
            "fatal: unable to access remote",
        ),
        (
            "stdout error after stderr notice",
            "Error: permission denied\nmore detail",
            "warning: update available",
            "Error: permission denied",
        ),
        (
            "compiler diagnostic",
            "",
            "compiling\nsource.rs:12: error: invalid syntax",
            "source.rs:12: error: invalid syntax",
        ),
        (
            "failure without error prefix",
            "update available\nFailed to connect to server\nsee documentation",
            "",
            "Failed to connect to server",
        ),
        (
            "unrecognised stderr retains precedence",
            "stdout detail",
            "\n stderr detail\nsecond detail",
            "stderr detail",
        ),
        (
            "unrecognised stdout fallback",
            "\n stdout detail\nsecond detail",
            "\n",
            "stdout detail",
        ),
    ];
    for (name, stdout, stderr, expected) in cases {
        let error = ExecError::non_zero("example", ExecResult::failure(stdout, stderr, Some(1)));
        assert_eq!(
            error.concise_message(),
            format!("example failed (exit 1): {expected}"),
            "{name}"
        );
        assert!(error.to_string().contains(stdout.trim()), "{name}");
        assert!(error.to_string().contains(stderr.trim()), "{name}");
    }
}

#[test]
fn empty_command_failure_output_has_no_diagnostic_suffix() {
    let error = ExecError::non_zero("example", ExecResult::failure(" \n", "\n", Some(1)));
    assert_eq!(error.concise_message(), "example failed (exit 1)");
}

#[test]
fn unchecked_nonzero_exit_is_neutral_but_checked_and_signal_failures_remain_failures() {
    use crate::infra::logging::records::{Record, StoredRecord};
    for (checked, code, expected) in [
        (false, Some(1), "nonzero_exit"),
        (true, Some(1), "failed"),
        (false, None, "failed"),
    ] {
        let (log, _tmp, _guard) = crate::infra::logging::isolated_logger();
        let result = ExecResult {
            stdout: "disabled\n".into(),
            stderr: String::new(),
            success: false,
            code,
        };
        log_command_output(
            "fixture-probe",
            &result,
            OutputLog::Diagnostics,
            Duration::ZERO,
            checked,
        );
        let content = std::fs::read_to_string(log.log_path().unwrap()).unwrap();
        let records: Vec<_> = content
            .lines()
            .filter_map(StoredRecord::from_line)
            .collect();
        assert_eq!(records.len(), 1);
        assert!(matches!(&records[0].record,
            Record::Command { outcome, exit_code, stdout: Some(stdout), .. }
            if outcome == expected && *exit_code == code && stdout == "disabled\n"));
    }
}
