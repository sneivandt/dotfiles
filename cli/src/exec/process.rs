//! Cross-platform child process-tree termination.

use std::process::Child;
use std::time::{Duration, Instant};

use super::POLL_INTERVAL;

const TERMINATION_GRACE: Duration = Duration::from_secs(2);

#[cfg(unix)]
pub(super) fn terminate_child(child: &Child) {
    terminate_process_tree(child);
}

#[cfg(windows)]
pub(super) fn terminate_child(child: &mut Child) {
    terminate_process_tree(child);
    if let Err(err) = child.kill()
        && err.kind() != std::io::ErrorKind::InvalidInput
    {
        tracing::debug!(target: "dotfiles::exec", "failed to kill child: {err}");
    }
}

#[cfg(unix)]
pub(super) fn wait_after_terminate(child: &mut Child) {
    let started = Instant::now();
    loop {
        match child_exited_without_reaping(child) {
            Ok(true) => break,
            Ok(false) if started.elapsed() >= TERMINATION_GRACE => break,
            Ok(false) => std::thread::sleep(POLL_INTERVAL),
            Err(err) => {
                tracing::debug!(target: "dotfiles::exec", "failed waiting after terminate: {err}");
                break;
            }
        }
    }

    // The leader has not been reaped, so its PID still identifies the original
    // process group and cannot be reused before this final escalation.
    force_kill_child(child);
    if let Err(err) = child.wait() {
        tracing::debug!(target: "dotfiles::exec", "failed waiting after force kill: {err}");
    }
}

#[cfg(windows)]
pub(super) fn wait_after_terminate(child: &mut Child) {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if started.elapsed() >= TERMINATION_GRACE => break,
            Ok(None) => std::thread::sleep(POLL_INTERVAL),
            Err(err) => {
                tracing::debug!(target: "dotfiles::exec", "failed waiting after terminate: {err}");
                return;
            }
        }
    }
    force_kill_child(child);
    if let Err(err) = child.wait() {
        tracing::debug!(target: "dotfiles::exec", "failed waiting after force kill: {err}");
    }
}

#[cfg(any(
    target_os = "android",
    target_os = "freebsd",
    target_os = "haiku",
    target_os = "linux"
))]
fn child_exited_without_reaping(child: &Child) -> nix::Result<bool> {
    use nix::sys::wait::{Id, WaitPidFlag, WaitStatus, waitid};

    let Ok(pid_raw) = i32::try_from(child.id()) else {
        return Ok(false);
    };
    let pid = nix::unistd::Pid::from_raw(pid_raw);
    let status = waitid(
        Id::Pid(pid),
        WaitPidFlag::WEXITED | WaitPidFlag::WNOHANG | WaitPidFlag::WNOWAIT,
    )?;
    Ok(matches!(
        status,
        WaitStatus::Exited(..) | WaitStatus::Signaled(..)
    ))
}

#[cfg(all(
    unix,
    not(any(
        target_os = "android",
        target_os = "freebsd",
        target_os = "haiku",
        target_os = "linux"
    ))
))]
fn child_exited_without_reaping(_child: &Child) -> nix::Result<bool> {
    Ok(false)
}

#[cfg(unix)]
fn terminate_process_tree(child: &Child) {
    signal_process_group(child, nix::sys::signal::Signal::SIGTERM);
}

#[cfg(windows)]
fn terminate_process_tree(child: &Child) {
    use std::process::{Command, Stdio};

    let pid = child.id().to_string();
    match Command::new("taskkill")
        .args(["/PID", &pid, "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => {
            tracing::debug!(target: "dotfiles::exec", "taskkill failed with status {status}");
        }
        Err(err) => {
            tracing::debug!(target: "dotfiles::exec", "failed to run taskkill: {err}");
        }
    }
}

#[cfg(unix)]
fn force_kill_child(child: &mut Child) {
    signal_process_group(child, nix::sys::signal::Signal::SIGKILL);
    if let Err(err) = child.kill()
        && err.kind() != std::io::ErrorKind::InvalidInput
    {
        tracing::debug!(target: "dotfiles::exec", "failed to force-kill child: {err}");
    }
}

#[cfg(windows)]
fn force_kill_child(child: &mut Child) {
    terminate_process_tree(child);
    if let Err(err) = child.kill()
        && err.kind() != std::io::ErrorKind::InvalidInput
    {
        tracing::debug!(target: "dotfiles::exec", "failed to force-kill child: {err}");
    }
}

#[cfg(unix)]
fn signal_process_group(child: &Child, signal: nix::sys::signal::Signal) {
    let Ok(pid_raw) = i32::try_from(child.id()) else {
        return;
    };
    let pid = nix::unistd::Pid::from_raw(pid_raw);
    if let Err(err) = nix::sys::signal::killpg(pid, signal) {
        tracing::debug!(target: "dotfiles::exec", "failed to signal process group {pid}: {err}");
    }
}
