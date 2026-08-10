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
fn run_failure() {
    let executor = ProcessExecutor::system();
    #[cfg(windows)]
    let result = executor.execute(CommandSpec::new("cmd").args(&["/C", "exit", "1"]));
    #[cfg(not(windows))]
    let result = executor.execute(CommandSpec::new("false"));
    assert!(
        matches!(result, Err(ExecError::NonZero { .. })),
        "non-zero exit should produce a typed error"
    );
}

#[test]
fn run_unchecked_failure() {
    let executor = ProcessExecutor::system();
    #[cfg(windows)]
    let result = executor
        .execute(
            CommandSpec::new("cmd")
                .args(&["/C", "exit", "1"])
                .unchecked(),
        )
        .unwrap();
    #[cfg(not(windows))]
    let result = executor
        .execute(CommandSpec::new("false").unchecked())
        .unwrap();
    assert!(!result.success, "non-zero exit should set success=false");
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

/// Repeated lookups are served from the memo table, so they must agree with
/// the first answer for both hits and misses.
#[test]
fn which_lookups_are_cached_consistently() {
    let executor = ProcessExecutor::system();
    #[cfg(windows)]
    let known = "cmd";
    #[cfg(not(windows))]
    let known = "echo";

    let first = executor.which_path(known).unwrap();
    let second = executor.which_path(known).unwrap();
    assert_eq!(first, second, "cached lookup should return the same path");
    assert!(executor.which(known), "cached hit should stay resolvable");

    let missing = "dotfiles-definitely-not-a-real-program";
    assert!(
        !executor.which(missing),
        "missing program should not resolve"
    );
    assert!(
        !executor.which(missing),
        "negative lookup should stay cached as missing"
    );
    assert!(executor.which_path(missing).is_err());
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
fn system_executor_which_path_finds_known_program() {
    let executor = ProcessExecutor::system();
    #[cfg(windows)]
    let result = executor.which_path("cmd");
    #[cfg(not(windows))]
    let result = executor.which_path("echo");
    assert!(
        result.is_ok(),
        "ProcessExecutor::which_path should find a known program"
    );
}

#[test]
fn system_executor_which_path_fails_for_missing() {
    let executor = ProcessExecutor::system();
    let result = executor.which_path("this-program-does-not-exist-12345");
    assert!(
        result.is_err(),
        "ProcessExecutor::which_path should fail for missing program"
    );
}

#[test]
fn run_in_tempdir() {
    let executor = ProcessExecutor::system();
    let dir = std::env::temp_dir();
    #[cfg(windows)]
    let result = executor
        .execute(
            CommandSpec::new("cmd")
                .args(&["/C", "echo", "hello"])
                .current_dir(&dir),
        )
        .unwrap();
    #[cfg(not(windows))]
    let result = executor
        .execute(CommandSpec::new("echo").arg("hello").current_dir(&dir))
        .unwrap();
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
fn managed_executor_cancels_commands() {
    let token = CancellationToken::new();
    token.cancel();
    let executor = ProcessExecutor::managed_with_timeout(token, Duration::from_secs(5));
    #[cfg(windows)]
    let result =
        executor.execute(CommandSpec::new("cmd").args(&["/C", "ping", "localhost", "-n", "5"]));
    #[cfg(not(windows))]
    let result = executor.execute(CommandSpec::new("sh").args(&["-c", "sleep 5"]));

    assert!(
        matches!(result, Err(ExecError::Cancelled { .. })),
        "cancelled command should produce a typed cancellation"
    );
}

#[test]
fn command_spec_builds_owned_request() {
    let dir = PathBuf::from("worktree");
    let spec = CommandSpec::new("git")
        .args(&["status", "--short"])
        .current_dir(&dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .unchecked();

    assert_eq!(spec.program(), "git");
    assert_eq!(spec.arguments(), ["status", "--short"]);
    assert_eq!(spec.working_dir(), Some(dir.as_path()));
    assert_eq!(
        spec.environment(),
        [(OsString::from("GIT_TERMINAL_PROMPT"), OsString::from("0"))]
    );
    assert!(
        !spec.is_checked(),
        "unchecked builder should disable checking"
    );
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
fn reader_failure_returns_typed_io_error() {
    let reader = std::thread::spawn(|| Err(io::Error::other("mock read failure")));
    let result = join_reader(reader, "reading stdout", "mock");

    assert!(
        matches!(result, Err(ExecError::Io { .. })),
        "output capture failure should produce a typed I/O error"
    );
}
