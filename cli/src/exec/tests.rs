//! Tests for command execution abstractions.

use super::*;

fn echo_result(msg: &str) -> Result<ExecResult> {
    let executor = SystemExecutor;
    #[cfg(windows)]
    {
        executor.run("cmd", &["/C", "echo", msg])
    }
    #[cfg(not(windows))]
    {
        executor.run("echo", &[msg])
    }
}

#[test]
fn run_echo() {
    let result = echo_result("hello").unwrap();
    assert!(result.success, "echo command should succeed");
    assert_eq!(result.stdout.trim(), "hello");
}

#[test]
fn run_failure() {
    let executor = SystemExecutor;
    #[cfg(windows)]
    let result = executor.run("cmd", &["/C", "exit", "1"]);
    #[cfg(not(windows))]
    let result = executor.run("false", &[]);
    assert!(result.is_err(), "non-zero exit should produce an error");
}

#[test]
fn run_unchecked_failure() {
    let executor = SystemExecutor;
    #[cfg(windows)]
    let result = executor.run_unchecked("cmd", &["/C", "exit", "1"]).unwrap();
    #[cfg(not(windows))]
    let result = executor.run_unchecked("false", &[]).unwrap();
    assert!(!result.success, "non-zero exit should set success=false");
}

#[test]
fn which_finds_known_program() {
    let executor = SystemExecutor;
    #[cfg(windows)]
    assert!(executor.which("cmd"), "cmd should be found on Windows");
    #[cfg(not(windows))]
    assert!(executor.which("echo"), "echo should be found on Unix");
}

#[test]
fn which_missing_program() {
    let executor = SystemExecutor;
    assert!(
        !executor.which("this-program-does-not-exist-12345"),
        "non-existent program should not be found"
    );
}

#[test]
fn which_path_finds_known_program() {
    let executor = SystemExecutor;
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
fn which_path_fails_for_missing_program() {
    let executor = SystemExecutor;
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
fn system_executor_which_path_finds_known_program() {
    let executor = SystemExecutor;
    #[cfg(windows)]
    let result = executor.which_path("cmd");
    #[cfg(not(windows))]
    let result = executor.which_path("echo");
    assert!(
        result.is_ok(),
        "SystemExecutor::which_path should find a known program"
    );
}

#[test]
fn system_executor_which_path_fails_for_missing() {
    let executor = SystemExecutor;
    let result = executor.which_path("this-program-does-not-exist-12345");
    assert!(
        result.is_err(),
        "SystemExecutor::which_path should fail for missing program"
    );
}

#[test]
fn run_in_tempdir() {
    let executor = SystemExecutor;
    let dir = std::env::temp_dir();
    #[cfg(windows)]
    let result = executor
        .run_in(&dir, "cmd", &["/C", "echo", "hello"])
        .unwrap();
    #[cfg(not(windows))]
    let result = executor.run_in(&dir, "echo", &["hello"]).unwrap();
    assert!(result.success, "echo in temp dir should succeed");
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
    let executor = ManagedExecutor {
        cancellation: token,
        timeout: Duration::from_millis(50),
    };
    #[cfg(windows)]
    let result = executor.run("cmd", &["/C", "ping", "localhost", "-n", "5"]);
    #[cfg(not(windows))]
    let result = executor.run("sh", &["-c", "sleep 5"]);

    let message = result
        .expect_err("long-running command should time out")
        .to_string();
    assert!(
        message.contains("timed out"),
        "timeout error should be explicit: {message}"
    );
}

#[test]
fn managed_executor_cancels_commands() {
    let token = CancellationToken::new();
    token.cancel();
    let executor = ManagedExecutor {
        cancellation: token,
        timeout: Duration::from_secs(5),
    };
    #[cfg(windows)]
    let result = executor.run("cmd", &["/C", "ping", "localhost", "-n", "5"]);
    #[cfg(not(windows))]
    let result = executor.run("sh", &["-c", "sleep 5"]);

    let message = result
        .expect_err("cancelled command should fail")
        .to_string();
    assert!(
        message.contains("cancelled"),
        "cancellation error should be explicit: {message}"
    );
}
