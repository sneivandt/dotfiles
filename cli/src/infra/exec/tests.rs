//! Tests for command execution abstractions.

use super::*;

fn echo_result(msg: &str) -> Result<ExecResult> {
    let executor = ProcessExecutor::system();
    #[cfg(windows)]
    {
        Ok(executor.execute(CommandSpec::new("cmd").args(&["/C", "echo", msg]))?)
    }
    #[cfg(not(windows))]
    {
        Ok(executor.execute(CommandSpec::new("echo").arg(msg))?)
    }
}

#[test]
fn run_echo() {
    let result = echo_result("hello").unwrap();
    assert!(result.success, "echo command should succeed");
    assert_eq!(result.stdout.trim(), "hello");
}

#[test]
fn checked_and_unchecked_failures_retain_exit_code_and_both_streams() {
    let executor = ProcessExecutor::system();
    for checked in [true, false] {
        #[cfg(windows)]
        let spec = CommandSpec::new("cmd").args(&[
            "/D",
            "/C",
            "(echo output)&(echo diagnostic 1>&2)&exit /b 7",
        ]);
        #[cfg(not(windows))]
        let spec = CommandSpec::new("sh").args(&[
            "-c",
            "printf 'output\\n'; printf 'diagnostic\\n' >&2; exit 7",
        ]);
        let label = spec.label();
        let result = executor.execute(if checked { spec } else { spec.unchecked() });
        let result = if checked {
            let ExecError::NonZero { command, result } = result.unwrap_err() else {
                panic!("checked failures must report NonZero");
            };
            assert_eq!(command, label);
            result
        } else {
            result.unwrap()
        };
        assert!(!result.success, "checked={checked}");
        assert_eq!(result.code, Some(7), "checked={checked}");
        assert_eq!(result.stdout.trim(), "output", "checked={checked}");
        assert_eq!(result.stderr.trim(), "diagnostic", "checked={checked}");
    }
}

#[test]
fn which_finds_known_program() {
    let executor = ProcessExecutor::system();
    #[cfg(windows)]
    assert!(executor.which("cmd"), "cmd should be found on Windows");
    #[cfg(not(windows))]
    assert!(executor.which("echo"), "echo should be found on Unix");
}

#[test]
fn which_missing_program() {
    let executor = ProcessExecutor::system();
    assert!(
        !executor.which("this-program-does-not-exist-12345"),
        "non-existent program should not be found"
    );
}

#[test]
fn which_path_finds_known_program() {
    let executor = ProcessExecutor::system();
    #[cfg(windows)]
    let result = executor.which_path("cmd");
    #[cfg(not(windows))]
    let result = executor.which_path("echo");
    assert!(result.is_ok(), "which_path should find a known program");
    let path = result.unwrap();
    assert!(
        path.is_absolute(),
        "which_path should return an absolute path"
    );
}

#[test]
#[cfg(unix)]
fn path_lookup_observes_an_executable_created_after_an_initial_miss() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("dotfiles-newly-installed-program");
    let program = executable.to_str().unwrap();
    let executor = ProcessExecutor::system();
    assert!(
        executor.which_path(program).is_err(),
        "program should initially be absent"
    );

    std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    let mut permissions = executable.metadata().unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();

    assert_eq!(
        executor.which_path(program).unwrap(),
        executable,
        "a fresh lookup must observe a newly installed executable"
    );
}

#[test]
fn which_path_fails_for_missing_program() {
    let executor = ProcessExecutor::system();
    let result = executor.which_path("this-program-does-not-exist-12345");
    assert!(
        result.is_err(),
        "which_path should fail for a missing program"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("not found on PATH"),
        "error message should mention 'not found on PATH'"
    );
}

#[test]
fn command_spec_applies_working_directory_and_child_only_environment_overrides() {
    let executor = ProcessExecutor::system();
    let dir = tempfile::tempdir().unwrap();
    let working_dir = crate::infra::fs::canonicalize(dir.path()).unwrap();
    let original_env = std::env::var_os("DOTFILES_EXEC_TEST_VALUE");
    #[cfg(windows)]
    let spec = CommandSpec::new("cmd").args(&["/D", "/C", "cd & echo %DOTFILES_EXEC_TEST_VALUE%"]);
    #[cfg(not(windows))]
    let spec = CommandSpec::new("sh").args(&[
        "-c",
        "printf '%s\\n' \"$PWD\" \"$DOTFILES_EXEC_TEST_VALUE\"",
    ]);
    let result = executor
        .execute(
            spec.current_dir(&working_dir)
                .env("DOTFILES_EXEC_TEST_VALUE", "overridden")
                .envs(&[("DOTFILES_EXEC_TEST_VALUE", "child-only value")]),
        )
        .unwrap();
    assert!(result.success);
    assert_eq!(result.code, Some(0));
    assert!(result.stderr.is_empty());
    let lines: Vec<_> = result.stdout.lines().map(str::trim).collect();
    assert_eq!(
        lines,
        [working_dir.to_str().unwrap(), "child-only value"],
        "both cwd and the last environment override must reach the child"
    );
    assert_eq!(std::env::var_os("DOTFILES_EXEC_TEST_VALUE"), original_env);
}

#[test]
fn stream_summary_ignores_blank_output() {
    assert_eq!(stream_summary("\n \n"), "");
}

#[test]
fn stream_summary_counts_non_empty_lines() {
    assert_eq!(stream_summary("one\n\n two \n"), "2 lines, 11 bytes");
}

#[test]
fn managed_executor_times_out_commands() {
    let token = CancellationToken::new();
    let executor = ProcessExecutor::managed_with_timeout(token, Duration::from_millis(50));
    #[cfg(windows)]
    let result =
        executor.execute(CommandSpec::new("cmd").args(&["/C", "ping", "localhost", "-n", "5"]));
    #[cfg(not(windows))]
    let result = executor.execute(CommandSpec::new("sh").args(&["-c", "sleep 5"]));

    assert!(
        matches!(result, Err(ExecError::TimedOut { .. })),
        "long-running command should produce a typed timeout"
    );
}

#[test]
fn command_spec_timeout_overrides_executor_default() {
    let executor = ProcessExecutor::system();
    #[cfg(windows)]
    let spec = CommandSpec::new("cmd")
        .args(&["/C", "ping", "localhost", "-n", "5"])
        .timeout(Duration::from_millis(50));
    #[cfg(not(windows))]
    let spec = CommandSpec::new("sh")
        .args(&["-c", "sleep 5"])
        .timeout(Duration::from_millis(50));

    assert!(
        matches!(executor.execute(spec), Err(ExecError::TimedOut { .. })),
        "per-command timeout should override the executor default"
    );
}

#[cfg(any(
    target_os = "android",
    target_os = "freebsd",
    target_os = "haiku",
    target_os = "linux"
))]
#[test]
fn timeout_remains_active_while_a_descendant_holds_output_pipes() {
    let executor =
        ProcessExecutor::managed_with_timeout(CancellationToken::new(), Duration::from_millis(100));
    let started = Instant::now();

    let result = executor.execute(CommandSpec::new("sh").args(&["-c", "sleep 5 & exit 0"]));

    assert!(
        matches!(result, Err(ExecError::TimedOut { .. })),
        "a descendant holding inherited pipes must not disable the timeout"
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the command should stop at the timeout plus the termination grace"
    );
}

#[test]
fn cancelled_executor_does_not_attempt_to_spawn_commands() {
    let token = CancellationToken::new();
    token.cancel();
    let executor = ProcessExecutor::managed(token);
    let error = executor
        .execute(CommandSpec::new(
            "dotfiles-this-program-does-not-exist-12345",
        ))
        .unwrap_err();

    let ExecError::Cancelled { result, .. } = error else {
        panic!("pre-cancelled commands must not reach process spawning: {error}");
    };
    assert!(result.stdout.is_empty());
    assert!(result.stderr.is_empty());
    assert!(!result.success);
    assert_eq!(result.code, None, "no process should have been started");
}

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
fn reader_failure_returns_typed_io_error() {
    for (reader, expected) in [
        (
            std::thread::spawn(|| Err(io::Error::other("mock read failure"))),
            "mock read failure",
        ),
        (
            std::thread::spawn(|| panic!("mock reader panic")),
            "output reader thread panicked",
        ),
    ] {
        let error = join_reader(reader, "reading stdout", "mock").unwrap_err();
        assert!(
            matches!(error, ExecError::Io { .. }),
            "output capture failure should produce a typed I/O error"
        );
        assert_eq!(
            error.to_string(),
            format!("reading stdout for mock: {expected}")
        );
        assert_eq!(error.io_error().unwrap().to_string(), expected);
    }
}

#[test]
fn output_retention_records_streams_and_preserves_multiline_boundaries() {
    use crate::infra::logging::records::{Record, StoredRecord};
    for (policy, success, keep_stdout, keep_stderr) in [
        (OutputLog::Diagnostics, true, false, true),
        (OutputLog::Diagnostics, false, true, true),
        (OutputLog::Full, true, true, true),
        (OutputLog::Omit, false, false, false),
    ] {
        let (log, _tmp, _guard) = crate::infra::logging::isolated_logger();
        let result = ExecResult {
            stdout: "stdout marker\n  second\n\n".into(),
            stderr: "\x1b[31mstderr marker\x1b[0m\n".into(),
            success,
            code: Some(i32::from(!success)),
        };
        log_command_output(
            "example [arguments redacted]",
            &result,
            policy,
            Duration::from_millis(123),
        );
        let content = std::fs::read_to_string(log.log_path().unwrap()).unwrap();
        let records: Vec<_> = content
            .lines()
            .filter_map(StoredRecord::from_line)
            .collect();
        assert_eq!(records.len(), 1, "{policy:?}: persist exactly one record");
        let record = records.into_iter().next().unwrap();
        let Record::Command {
            command,
            outcome,
            stdout,
            stderr,
            exit_code,
            elapsed_us,
            stdout_bytes,
            stderr_bytes,
        } = record.record
        else {
            panic!("expected command record")
        };
        assert_eq!(stdout.is_some(), keep_stdout, "{policy:?}");
        assert_eq!(stderr.is_some(), keep_stderr, "{policy:?}");
        if keep_stdout {
            assert_eq!(stdout.as_deref(), Some(result.stdout.as_str()));
        }
        if keep_stderr {
            assert_eq!(stderr.as_deref(), Some("stderr marker\n"));
        }
        assert_eq!(exit_code, result.code);
        assert_eq!(command, "example [arguments redacted]");
        assert_eq!(outcome, if success { "succeeded" } else { "failed" });
        assert_eq!(elapsed_us, 123_000);
        assert_eq!(stdout_bytes, result.stdout.len());
        assert_eq!(stderr_bytes, result.stderr.len());
        assert!(!content.contains("\\u001b"));
    }
}

#[test]
fn omitted_unchecked_output_is_available_to_caller_but_not_persisted() {
    use crate::infra::logging::records::{Record, StoredRecord};
    let (log, _tmp, _guard) = crate::infra::logging::isolated_logger();
    #[cfg(windows)]
    let spec =
        CommandSpec::new("cmd").args(&["/D", "/C", "(echo synthetic-private-output)&exit /b 3"]);
    #[cfg(not(windows))]
    let spec = CommandSpec::new("sh").args(&["-c", "printf 'synthetic-private-output\\n'; exit 3"]);
    let result = ProcessExecutor::system()
        .execute(
            spec.unchecked()
                .redact_arguments()
                .output_log(OutputLog::Omit),
        )
        .unwrap();
    assert_eq!(result.code, Some(3));
    assert!(!result.success);
    assert_eq!(result.stdout.trim(), "synthetic-private-output");
    let content = std::fs::read_to_string(log.log_path().unwrap()).unwrap();
    assert!(!content.contains("synthetic-private-output"));
    let records: Vec<_> = content
        .lines()
        .filter_map(StoredRecord::from_line)
        .collect();
    assert_eq!(records.len(), 1);
    assert!(matches!(
        &records[0].record,
        Record::Command { stdout: None, stderr: None, stdout_bytes, exit_code: Some(3), outcome, .. }
            if *stdout_bytes == result.stdout.len() && outcome == "failed"
    ));
}

#[test]
fn omitted_failure_output_does_not_escape_through_the_error() {
    let (log, _tmp, _guard) = crate::infra::logging::isolated_logger();
    #[cfg(windows)]
    let spec = CommandSpec::new("cmd").args(&["/C", "echo synthetic-sensitive-marker & exit /b 3"]);
    #[cfg(not(windows))]
    let spec = CommandSpec::new("sh").args(&["-c", "echo synthetic-sensitive-marker; exit 3"]);
    let error = ProcessExecutor::system()
        .execute(spec.redact_arguments().output_log(OutputLog::Omit))
        .unwrap_err();
    assert!(!error.to_string().contains("synthetic-sensitive-marker"));
    let content = std::fs::read_to_string(log.log_path().unwrap()).unwrap();
    assert!(!content.contains("synthetic-sensitive-marker"));
    assert!(content.contains("omitted") || content.contains("stdout_bytes"));
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
