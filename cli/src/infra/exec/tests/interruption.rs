use super::*;

#[cfg(any(
    target_os = "android",
    target_os = "freebsd",
    target_os = "haiku",
    target_os = "linux"
))]
#[test]
fn bounded_command_wait_detects_exit_without_reaping() {
    use nix::sys::wait::{Id, WaitPidFlag, waitid};

    let mut child = Command::new("sh")
        .args(["-c", "read -r input; exit 17"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let running = child_exited_without_reaping(&child);
    drop(child.stdin.take());
    let pid = nix::unistd::Pid::from_raw(i32::try_from(child.id()).unwrap());
    let _status = waitid(Id::Pid(pid), WaitPidFlag::WEXITED | WaitPidFlag::WNOWAIT).unwrap();
    let exited = child_exited_without_reaping(&child);
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(17));
    assert!(!running.unwrap());
    assert!(exited.unwrap());
}

#[cfg(unix)]
#[test]
fn bounded_command_wait_captures_output_and_times_out_slow_children() {
    let executor =
        ProcessExecutor::managed_with_timeout(CancellationToken::new(), Duration::from_millis(500));
    let complete = executor
        .execute(CommandSpec::new("sh").args(&["-c", "printf marker"]))
        .unwrap();
    assert_eq!(complete.stdout, "marker");

    let slow = executor.execute(CommandSpec::new("sh").args(&["-c", "sleep 2"]));
    assert!(matches!(slow, Err(ExecError::TimedOut { .. })), "{slow:?}");
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

#[cfg(unix)]
#[test]
fn interrupting_detached_pipe_writers_preserves_output_and_returns_promptly() {
    for cancelled in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let mut command = new_command(std::env::current_exe().unwrap());
        detached_pipe_command(&mut command, fixture.path(), "leader");
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = process::spawn(command).unwrap();
        let ready = fixture.path().join("ready");
        let ready_started = Instant::now();
        while !ready.exists() && ready_started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(POLL_INTERVAL);
        }
        if !ready.exists() {
            terminate_child(&child);
            wait_after_terminate(&mut child);
            panic!("descendant did not detach and write its markers");
        }
        let token = CancellationToken::new();
        if cancelled {
            token.cancel();
        }
        let settings = CommandSettings::managed(token, Duration::ZERO);
        let started = Instant::now();
        let error = collect_child(child, "detached pipe fixture", &settings).unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "detached writer held capture open after interruption: {error:?}"
        );
        let result = if cancelled {
            let ExecError::Cancelled { result, .. } = error else {
                panic!("wrong cancellation classification: {error:?}");
            };
            result
        } else {
            let ExecError::TimedOut { result, .. } = error else {
                panic!("wrong timeout classification: {error:?}");
            };
            result
        };
        assert!(result.stdout.contains("detached stdout"));
        assert!(result.stderr.contains("detached stderr"));
    }
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
fn interrupting_a_started_child_preserves_output_and_never_reports_an_exit_code() {
    for cancelled in [false, true] {
        let mut command = new_command(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "infra::exec::tests::interruption_child",
                "--nocapture",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = process::spawn(command).unwrap();
        let mut stderr = process::take_stderr(&mut child).unwrap();
        let (ready, received) = channel();
        let reader = std::thread::spawn(move || {
            let mut marker = [0; 6];
            let read = stderr.read_exact(&mut marker);
            ready.send((stderr, marker, read)).unwrap();
        });
        match received.recv_timeout(Duration::from_secs(10)) {
            Ok((captured_stderr, marker, Ok(()))) if marker == *b"ready\n" => {
                #[cfg(unix)]
                {
                    child.stderr = Some(captured_stderr);
                }
                #[cfg(windows)]
                {
                    *child.stderr() = Some(captured_stderr);
                }
            }
            readiness => {
                #[cfg(unix)]
                terminate_child(&child);
                #[cfg(windows)]
                terminate_child(&mut child);
                wait_after_terminate(&mut child);
                reader.join().unwrap();
                panic!("interruption fixture did not become ready: {readiness:?}");
            }
        }
        reader.join().unwrap();

        let token = CancellationToken::new();
        if cancelled {
            token.cancel();
        }
        let settings = CommandSettings::managed(
            token,
            if cancelled {
                Duration::from_secs(5)
            } else {
                Duration::ZERO
            },
        );
        let error = collect_child(child, "interruption fixture", &settings).unwrap_err();
        assert_eq!(error.command(), "interruption fixture");
        assert_eq!(error.is_cancelled(), cancelled);
        let result = if cancelled {
            let ExecError::Cancelled { result, .. } = error else {
                panic!("wrong interruption classification: {error:?}");
            };
            result
        } else {
            let ExecError::TimedOut {
                result, timeout, ..
            } = error
            else {
                panic!("wrong interruption classification: {error:?}");
            };
            assert_eq!(timeout, Duration::ZERO);
            result
        };
        assert!(!result.success);
        assert_eq!(result.code, None);
        assert!(
            result.stdout.contains("captured before interruption\n"),
            "output written before readiness must survive child termination"
        );
    }
}
