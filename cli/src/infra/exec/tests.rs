//! Tests for command execution abstractions.

use super::*;

#[cfg(unix)]
const DETACHED_PIPE_TEST: &str = "infra::exec::tests::detached_pipe_child";

#[cfg(unix)]
fn detached_pipe_command(command: &mut Command, fixture: &Path, role: &str) {
    command
        .args(["--ignored", "--exact", DETACHED_PIPE_TEST, "--nocapture"])
        .env("DOTFILES_EXEC_PIPE_FIXTURE", fixture)
        .env("DOTFILES_EXEC_PIPE_ROLE", role);
}

#[cfg(unix)]
#[test]
#[ignore = "subprocess fixture for the detached pipe test"]
fn detached_pipe_child() {
    use std::io::Write as _;

    let fixture = PathBuf::from(std::env::var_os("DOTFILES_EXEC_PIPE_FIXTURE").unwrap());
    if std::env::var("DOTFILES_EXEC_PIPE_ROLE").unwrap() == "leader" {
        let mut command = Command::new(std::env::current_exe().unwrap());
        detached_pipe_command(&mut command, &fixture, "descendant");
        drop(command.spawn().unwrap());
        return;
    }
    nix::unistd::setsid().unwrap();
    io::stdout().write_all(b"detached stdout\n").unwrap();
    io::stderr().write_all(b"detached stderr\n").unwrap();
    std::fs::create_dir(fixture.join("ready")).unwrap();
    // Parent cleanup releases the fixture. Keep a deadline for the failure
    // case so the test also finishes against a blocking reader.
    let started = Instant::now();
    while fixture.exists() && started.elapsed() < Duration::from_secs(6) {
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[test]
#[ignore = "subprocess fixture launched by the interruption test"]
fn interruption_child() {
    use std::io::Write as _;

    io::stdout()
        .write_all(b"captured before interruption\n")
        .unwrap();
    io::stdout().flush().unwrap();
    io::stderr().write_all(b"ready\n").unwrap();
    io::stderr().flush().unwrap();
    let mut input = [0];
    io::stdin().read_exact(&mut input).unwrap();
    panic!("the parent must terminate this child before releasing stdin");
}

#[test]
#[ignore = "subprocess fixture launched by output capture tests"]
fn output_capture_child() {
    use std::io::Write as _;
    let stream = std::env::var("DOTFILES_EXEC_CAPTURE_STREAM").unwrap();
    let bytes = vec![b'x'; MAX_CAPTURE_BYTES + 1];
    match stream.as_str() {
        "stdout" => io::stdout().lock().write_all(&bytes).unwrap(),
        "stderr" => io::stderr().lock().write_all(&bytes).unwrap(),
        "both" => {
            io::stdout().lock().write_all(&bytes).unwrap();
            io::stderr().lock().write_all(&bytes).unwrap();
        }
        _ => panic!("unknown stream fixture"),
    }
}

mod capture;
mod commands;
mod diagnostics;
mod interruption;
