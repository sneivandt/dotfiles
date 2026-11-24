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

#[cfg(unix)]
#[test]
fn terminate_child_sends_sigterm_and_wait_escalates_to_sigkill_for_process_group() {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;

    let fixture = tempfile::tempdir().unwrap();

    // 1. A cooperative child trapping SIGTERM exits promptly before the grace window.
    let term_marker = fixture.path().join("sigterm");
    let term_ready = fixture.path().join("sigterm-ready");
    let mut graceful_cmd = new_command("sh");
    graceful_cmd
        .args([
            "-c",
            "trap 'printf got > \"$1\"; exit 0' TERM; printf ready > \"$2\"; while :; do sleep 0.05; done",
            "sh",
            term_marker.to_str().unwrap(),
            term_ready.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut graceful = process::spawn(graceful_cmd).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !term_ready.exists() && Instant::now() < deadline {
        std::thread::sleep(POLL_INTERVAL);
    }
    assert!(term_ready.exists(), "graceful fixture did not start");
    let started = Instant::now();
    terminate_child(&graceful);
    wait_after_terminate(&mut graceful);
    assert_eq!(
        std::fs::read_to_string(&term_marker).unwrap_or_default(),
        "got",
        "terminate_child must deliver SIGTERM to the child process group"
    );
    assert!(
        started.elapsed() < process::TERMINATION_GRACE,
        "cooperative child should exit before the termination grace expires"
    );

    // 2. A stubborn child ignoring SIGTERM with a background descendant must
    // wait out the grace window and then have its entire process group SIGKILLed.
    let child_pid_file = fixture.path().join("descendant.pid");
    let mut stubborn_cmd = new_command("sh");
    stubborn_cmd
        .args([
            "-c",
            "trap '' TERM; sleep 30 & echo $! > \"$1\"; wait",
            "sh",
            child_pid_file.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut stubborn = process::spawn(stubborn_cmd).unwrap();
    let pid_deadline = Instant::now() + Duration::from_secs(5);
    while std::fs::read_to_string(&child_pid_file).is_ok_and(|s| s.trim().is_empty())
        || !child_pid_file.exists()
    {
        if Instant::now() >= pid_deadline {
            break;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    let descendant_pid: i32 = std::fs::read_to_string(&child_pid_file)
        .expect("descendant pid file")
        .trim()
        .parse()
        .expect("numeric descendant pid");
    let kill_started = Instant::now();
    terminate_child(&stubborn);
    wait_after_terminate(&mut stubborn);
    assert!(
        kill_started.elapsed() >= process::TERMINATION_GRACE,
        "wait_after_terminate must wait through TERMINATION_GRACE before force-killing"
    );
    assert!(
        stubborn.try_wait().unwrap().is_some(),
        "wait_after_terminate must reap the leader after force-killing"
    );
    // Allow brief kernel cleanup of the SIGKILLed descendant.
    let kill_deadline = Instant::now() + Duration::from_secs(2);
    while kill(Pid::from_raw(descendant_pid), None).is_ok() && Instant::now() < kill_deadline {
        std::thread::sleep(POLL_INTERVAL);
    }
    assert!(
        kill(Pid::from_raw(descendant_pid), None).is_err(),
        "force_kill_child must SIGKILL the child process group including descendants"
    );
}
