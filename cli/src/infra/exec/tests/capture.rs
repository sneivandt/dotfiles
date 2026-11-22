use super::*;

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
fn output_reader_caps_retention_and_drains_the_entire_stream() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct CountedReader {
        remaining: usize,
        read: Arc<AtomicUsize>,
    }

    impl Read for CountedReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let count = buffer.len().min(self.remaining);
            buffer.split_at_mut(count).0.fill(b'x');
            self.remaining -= count;
            self.read.fetch_add(count, Ordering::Relaxed);
            Ok(count)
        }
    }

    for length in [
        0,
        MAX_CAPTURE_BYTES - 1,
        MAX_CAPTURE_BYTES,
        MAX_CAPTURE_BYTES + 1,
        MAX_CAPTURE_BYTES * 2,
    ] {
        let read = Arc::new(AtomicUsize::new(0));
        let (done, closed) = channel();
        let reader = spawn_reader(
            CountedReader {
                remaining: length,
                read: Arc::clone(&read),
            },
            done,
        );
        let bytes = reader.join().unwrap().unwrap();
        assert_eq!(bytes.len(), length.min(MAX_CAPTURE_BYTES + 1), "{length}");
        assert_eq!(
            read.load(Ordering::Relaxed),
            length,
            "must drain {length} bytes"
        );
        assert_eq!(
            closed.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        );
        let text = decode_captured_output(bytes);
        assert_eq!(
            text.contains("[output truncated:"),
            length > MAX_CAPTURE_BYTES,
            "{length}"
        );
    }
}

#[test]
fn output_decoding_reuses_valid_utf8_and_replaces_invalid_bytes() {
    let bytes = b"valid UTF-8 \xc3\xa9".to_vec();
    let original = bytes.as_ptr();
    let text = decode_output(bytes);
    assert_eq!(text, "valid UTF-8 \u{e9}");
    assert_eq!(text.as_ptr(), original, "valid buffers must not be copied");
    assert_eq!(decode_output(vec![b'a', 0xff, b'b']), "a\u{fffd}b");
}

#[test]
fn overflowing_output_is_an_error_even_for_unchecked_commands() {
    for (stream, checked, policy) in [
        ("stdout", true, OutputLog::Diagnostics),
        ("stderr", false, OutputLog::Full),
        ("both", false, OutputLog::Omit),
    ] {
        let (log, _tmp, _guard) = crate::infra::logging::isolated_logger();
        let spec = CommandSpec::new(std::env::current_exe().unwrap())
            .args(&[
                "--ignored",
                "--exact",
                "infra::exec::tests::output_capture_child",
                "--nocapture",
            ])
            .env("DOTFILES_EXEC_CAPTURE_STREAM", stream)
            .output_log(policy);
        let label = spec.label();
        let error = ProcessExecutor::system()
            .execute(if checked { spec } else { spec.unchecked() })
            .unwrap_err();
        assert_eq!(error.command(), label);
        assert!(error.concise_message().contains("capture limit"));
        assert!(error.io_error().is_none());
        let ExecError::OutputLimit {
            result,
            limit,
            stream: exceeded,
            ..
        } = error
        else {
            panic!("overflow must produce a typed capture error")
        };
        assert_eq!(limit, MAX_CAPTURE_BYTES);
        assert_eq!(exceeded, if stream == "both" { "stdout" } else { stream });
        assert_eq!(
            result.code,
            Some(0),
            "the child must drain and exit normally"
        );
        if policy == OutputLog::Omit {
            assert_eq!(result.stdout, "[output omitted]");
            assert_eq!(result.stderr, "[output omitted]");
        } else {
            let text = if exceeded == "stdout" {
                &result.stdout
            } else {
                &result.stderr
            };
            assert!(text.ends_with("[output truncated: capture limit exceeded]"));
            assert!(text.len() < MAX_CAPTURE_BYTES + 100);
        }
        let retained = std::fs::read_to_string(log.log_path().unwrap()).unwrap();
        assert!(retained.contains("\"outcome\":\"output_limit\""));
        if policy == OutputLog::Omit {
            assert!(!retained.contains(&"x".repeat(100)));
        }
    }
}

#[test]
fn terminated_capture_keeps_bounded_diagnostics_without_reclassifying_interruption() {
    let stdout = std::thread::spawn(|| Ok(vec![b'x'; MAX_CAPTURE_BYTES + 1]));
    let stderr = std::thread::spawn(|| Ok(Vec::new()));
    let result = collect_result(None, stdout, stderr, "terminated").unwrap();
    assert!(!result.success);
    assert_eq!(result.code, None);
    assert!(
        result
            .stdout
            .ends_with("[output truncated: capture limit exceeded]")
    );
    assert!(result.stdout.len() < MAX_CAPTURE_BYTES + 100);
}

#[test]
fn completed_capture_accepts_the_exact_limit_and_rejects_one_extra_byte() {
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt as _;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt as _;

    assert_eq!(MAX_CAPTURE_BYTES, 8_388_608);
    for stream in ["stdout", "stderr"] {
        for length in [MAX_CAPTURE_BYTES, MAX_CAPTURE_BYTES + 1] {
            let stdout = std::thread::spawn(move || {
                Ok(if stream == "stdout" {
                    vec![b'x'; length]
                } else {
                    Vec::new()
                })
            });
            let stderr = std::thread::spawn(move || {
                Ok(if stream == "stderr" {
                    vec![b'x'; length]
                } else {
                    Vec::new()
                })
            });
            let result = collect_result(
                Some(std::process::ExitStatus::from_raw(0)),
                stdout,
                stderr,
                "boundary",
            );
            if length == MAX_CAPTURE_BYTES {
                let result = result.unwrap();
                assert!(result.success);
                assert_eq!(result.stdout.len() + result.stderr.len(), MAX_CAPTURE_BYTES);
            } else {
                assert!(
                    matches!(result, Err(ExecError::OutputLimit { stream: exceeded, .. }) if exceeded == stream)
                );
            }
        }
    }
}

#[test]
fn output_retention_records_streams_and_preserves_multiline_boundaries() {
    use crate::infra::logging::records::{Record, StoredRecord};
    for (policy, success, keep_stdout, keep_stderr) in [
        (OutputLog::Diagnostics, true, false, true),
        (OutputLog::Diagnostics, false, true, true),
        (OutputLog::Full, true, true, true),
        (OutputLog::Full, false, true, true),
        (OutputLog::Omit, true, false, false),
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
            true,
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
fn interrupted_output_retention_preserves_classification_and_redacts_both_error_streams() {
    use crate::infra::logging::records::{Record, StoredRecord};

    for policy in [OutputLog::Diagnostics, OutputLog::Full, OutputLog::Omit] {
        let captured = ExecResult::failure("partial stdout\n", "partial stderr\n", None);
        for (mut error, expected_outcome) in [
            (
                ExecError::Cancelled {
                    command: "fixture".into(),
                    result: captured.clone(),
                },
                "interrupted",
            ),
            (
                ExecError::TimedOut {
                    command: "fixture".into(),
                    timeout: Duration::from_secs(2),
                    result: captured.clone(),
                },
                "timed_out",
            ),
            (
                ExecError::OutputLimit {
                    command: "fixture".into(),
                    stream: "stderr",
                    limit: MAX_CAPTURE_BYTES,
                    result: captured.clone(),
                },
                "output_limit",
            ),
        ] {
            let (log, _root, _guard) = crate::infra::logging::isolated_logger();
            output::log_command_error("fixture", &error, policy, Duration::from_micros(123));
            let content = std::fs::read_to_string(log.log_path().unwrap()).unwrap();
            let records: Vec<_> = content
                .lines()
                .filter_map(StoredRecord::from_line)
                .collect();
            assert_eq!(records.len(), 1, "{policy:?}: {expected_outcome}");
            let Record::Command {
                command,
                outcome,
                exit_code,
                elapsed_us,
                stdout,
                stderr,
                stdout_bytes,
                stderr_bytes,
            } = &records[0].record
            else {
                panic!("expected a command record");
            };
            assert_eq!(command, "fixture");
            assert_eq!(outcome, expected_outcome);
            assert_eq!(*exit_code, None);
            assert_eq!(*elapsed_us, 123);
            assert_eq!(*stdout_bytes, captured.stdout.len());
            assert_eq!(*stderr_bytes, captured.stderr.len());
            if policy == OutputLog::Omit {
                assert_eq!((stdout, stderr), (&None, &None));
                assert!(!content.contains("partial stdout"));
                assert!(!content.contains("partial stderr"));
                error.omit_output();
                let result = match &error {
                    ExecError::Cancelled { result, .. }
                    | ExecError::TimedOut { result, .. }
                    | ExecError::OutputLimit { result, .. } => result,
                    ExecError::Spawn { .. } | ExecError::Io { .. } | ExecError::NonZero { .. } => {
                        panic!("redaction changed the error type: {error:?}")
                    }
                };
                assert_eq!(result.stdout, "[output omitted]");
                assert_eq!(result.stderr, "[output omitted]");
                assert!(!result.success);
                assert_eq!(result.code, None);
                assert!(!error.to_string().contains("partial"));
                assert!(!error.concise_message().contains("partial"));
            } else {
                assert_eq!(stdout.as_deref(), Some("partial stdout\n"));
                assert_eq!(stderr.as_deref(), Some("partial stderr\n"));
                assert!(error.to_string().contains("partial stdout"));
                assert!(error.to_string().contains("partial stderr"));
            }
        }
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
            if *stdout_bytes == result.stdout.len() && outcome == "nonzero_exit"
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
