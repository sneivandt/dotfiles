//! Native job-object regressions with an exited leader and inherited pipes.

use super::*;

const CHILD_TEST: &str = "infra::exec::tests_windows::pipe_child";
const CHILD_ROLE: &str = "DOTFILES_EXEC_PIPE_TEST_ROLE";

fn fixture_command(role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--ignored", "--exact", CHILD_TEST, "--nocapture"])
        .env(CHILD_ROLE, role)
        .stdin(Stdio::null());
    command
}

#[test]
#[ignore = "subprocess fixture launched by the native process-tree tests"]
#[allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "the fixture supplies captured output to the executor under test"
)]
fn pipe_child() {
    let role = std::env::var(CHILD_ROLE).unwrap();
    if let Some(streams) = role.strip_prefix("leader-") {
        let mut command = fixture_command(if streams == "short" { "short" } else { "long" });
        if streams == "stdout" {
            command.stderr(Stdio::null());
        } else if streams == "stderr" {
            command.stdout(Stdio::null());
        }
        // The outer test's job owns this descendant after this leader exits.
        drop(command.spawn().unwrap());
        println!("leader finished");
    } else {
        println!("descendant stdout");
        eprintln!("descendant stderr");
        std::thread::sleep(if role == "short" {
            Duration::from_millis(100)
        } else {
            Duration::from_secs(5)
        });
        println!("descendant finished");
    }
}

fn exited_leader(streams: &str) -> Child {
    let mut command = fixture_command(&format!("leader-{streams}"));
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = process::spawn(command).unwrap();
    // Wait only for the native leader, not the surrounding job. This makes
    // the inherited-pipe race deterministic before collection starts.
    assert!(
        child.inner_mut().wait().unwrap().success(),
        "fixture leader must exit successfully"
    );
    child
}

#[test]
fn timeout_terminates_descendants_after_the_leader_exits() {
    for streams in ["stdout", "stderr", "both"] {
        let child = exited_leader(streams);
        let settings = CommandSettings::timeout(Duration::from_millis(100));
        let started = Instant::now();
        let result = collect_child(child, "pipe fixture", &settings);

        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{streams}: collection must not wait for the five-second descendant"
        );
        let Err(ExecError::TimedOut { result, .. }) = result else {
            panic!("{streams}: inherited pipes must remain subject to the deadline: {result:?}");
        };
        assert!(
            result.stdout.contains("leader finished"),
            "{streams}: output captured before termination must be preserved"
        );
    }
}

#[test]
fn cancellation_terminates_descendants_after_the_leader_exits() {
    let child = exited_leader("both");
    let token = CancellationToken::new();
    token.cancel();
    let settings = CommandSettings::managed(token, Duration::from_secs(10));
    let started = Instant::now();
    let result = collect_child(child, "pipe fixture", &settings);

    assert!(
        matches!(result, Err(ExecError::Cancelled { .. })),
        "cancellation must terminate the job even after its leader exits: {result:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "cancellation must not wait for the five-second descendant"
    );
}

#[test]
fn successful_collection_drains_descendants_after_the_leader_exits() {
    let child = exited_leader("short");
    let result = collect_child(
        child,
        "pipe fixture",
        &CommandSettings::timeout(Duration::from_secs(5)),
    )
    .unwrap();

    assert!(result.success, "preserve the successful leader status");
    assert_eq!(result.code, Some(0));
    assert!(result.stdout.contains("descendant stdout"));
    assert!(result.stdout.contains("descendant finished"));
    assert!(result.stderr.contains("descendant stderr"));
}
