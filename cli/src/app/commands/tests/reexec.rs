use super::*;
use crate::infra::exec::{CommandSpec, Executor as _, ProcessExecutor};
use crate::infra::logging::records::{Record, RunOutcome, StoredRecord};

const CHILD_TEST: &str = "app::commands::reexec::tests::restart_child";
const CHILD_ROLE: &str = "DOTFILES_RESTART_TEST_ROLE";
const CHILD_ROOT: &str = "DOTFILES_RESTART_TEST_ROOT";
#[cfg(unix)]
const CHILD_PARENT_PID: &str = "DOTFILES_RESTART_TEST_PPID";

#[test]
#[ignore = "subprocess fixture launched by restart outcome tests"]
fn restart_child() {
    let role = std::env::var(CHILD_ROLE).unwrap();
    if let Some(code) = role.strip_prefix("exit-") {
        std::process::exit(code.parse().unwrap());
    }
    #[cfg(unix)]
    if role == "signal" {
        nix::sys::signal::raise(nix::sys::signal::Signal::SIGTERM).unwrap();
        panic!("SIGTERM must terminate the replacement process");
    }

    let root = std::path::PathBuf::from(std::env::var_os(CHILD_ROOT).unwrap());
    if matches!(role.as_str(), "output" | "output-tail") {
        let global = super::super::tests::global(&[]);
        let runtime = RuntimePolicy::new(&global, false, crate::infra::env::system(), false, false);
        let mut log = crate::infra::logging::Logger::new_in("update", &root.join(&role));
        runtime.restore_output(&mut log).unwrap();
        if role == "output" {
            log.startup("Update · fixture");
        }
        log.print_summary(crate::infra::logging::CompletionDisposition::Succeeded);
        std::fs::write(
            root.join(format!("child-{role}")),
            log.captured_lines().join("\n"),
        )
        .unwrap();
        if role == "output" {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args(["--ignored", "--exact", CHILD_TEST, "--nocapture"])
                .env(CHILD_ROLE, "output-tail");
            run_reexec(command, &root, None, &log);
        }
        return;
    }
    #[cfg(unix)]
    if role == "interrupt" {
        let expected_ppid: i32 = std::env::var(CHILD_PARENT_PID)
            .expect("expected parent PID for interrupt fixture")
            .parse()
            .expect("numeric parent PID");
        let log = std::sync::Arc::new(crate::infra::logging::Logger::new_in(
            "update",
            &root.join("child"),
        ));
        let token = crate::engine::CancellationToken::new();
        crate::app::interrupt::install(&token, &log, true);
        for index in 0..2 {
            // Verify the test parent is still our parent before signaling
            // SIGINT; if a mutant caused the parent to exit early, our ppid
            // would belong to an ancestor subreaper (e.g., nextest/CI runner).
            let ppid = nix::unistd::getppid();
            assert_eq!(
                ppid.as_raw(),
                expected_ppid,
                "parent process exited early; refusing to signal reparented ancestor"
            );
            nix::sys::signal::kill(ppid, nix::sys::signal::Signal::SIGINT).unwrap();
            nix::sys::signal::raise(nix::sys::signal::Signal::SIGINT).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            if index == 0 {
                while !token.is_cancelled() {
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::yield_now();
                }
                std::fs::write(root.join("child-completed-work"), "preserved").unwrap();
            } else {
                while std::time::Instant::now() < deadline {
                    std::thread::yield_now();
                }
                panic!("child must handle its second interrupt");
            }
        }
    }
    let mut log = crate::infra::logging::Logger::new_in("update", &root);
    if matches!(role.as_str(), "parent-output" | "missing") {
        log.set_invocation_start(log.invocation_start() - std::time::Duration::from_secs(60))
            .unwrap();
        log.queue_startup_transition("CLI upgraded · old → new".into());
    }
    let log = std::sync::Arc::new(log);
    if role == "parent-interrupt" {
        crate::app::interrupt::install(&crate::engine::CancellationToken::new(), &log, true);
    }
    let run = log.run_log().unwrap();
    run.start_run("update", None);
    std::fs::write(root.join("run-path"), run.path().to_str().unwrap()).unwrap();
    let mut command = if role == "missing" {
        std::process::Command::new(root.join("missing-executable"))
    } else {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--ignored", "--exact", CHILD_TEST, "--nocapture"])
            .env(CHILD_ROLE, role.strip_prefix("parent-").unwrap());
        #[cfg(unix)]
        if role == "parent-interrupt" {
            command.env(CHILD_PARENT_PID, std::process::id().to_string());
        }
        command
    };
    command.stdin(std::process::Stdio::null());
    git2::Repository::init(&root).unwrap();
    run_reexec(command, &root, None, &log);
}

#[test]
fn restarted_process_exit_and_spawn_failure_finish_the_parent_run() {
    let cases = vec![
        ("parent-exit-0", 0, RunOutcome::Succeeded, None),
        ("parent-exit-23", 23, RunOutcome::Failed, None),
        ("parent-exit-130", 130, RunOutcome::Interrupted, None),
        ("missing", 1, RunOutcome::Failed, Some("failed to re-exec")),
        ("parent-output", 0, RunOutcome::Succeeded, None),
    ];
    #[cfg(unix)]
    let cases = {
        let mut cases = cases;
        cases.push((
            "parent-signal",
            1,
            RunOutcome::Failed,
            Some("child process terminated by signal"),
        ));
        cases.push(("parent-interrupt", 130, RunOutcome::Interrupted, None));
        cases
    };
    for (role, code, expected_outcome, diagnostic) in cases {
        let fixture = tempfile::tempdir().unwrap();
        let result = ProcessExecutor::system()
            .execute(
                CommandSpec::new(std::env::current_exe().unwrap())
                    .args(&["--ignored", "--exact", CHILD_TEST, "--nocapture"])
                    .env(CHILD_ROLE, role)
                    .env(CHILD_ROOT, fixture.path().as_os_str())
                    .timeout(std::time::Duration::from_secs(20))
                    .unchecked(),
            )
            .unwrap();
        assert_eq!(result.code, Some(code), "{role}: {result:?}");
        let path = std::fs::read_to_string(fixture.path().join("run-path")).unwrap();
        let contents = std::fs::read_to_string(path).unwrap();
        let finishes = contents
            .lines()
            .filter_map(StoredRecord::from_line)
            .filter_map(|stored| {
                if let Record::RunFinish {
                    outcome, exit_code, ..
                } = stored.record
                {
                    Some((outcome, exit_code))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(finishes, [(expected_outcome, code)], "{role}: {contents}");
        if let Some(diagnostic) = diagnostic {
            assert!(contents.contains(diagnostic), "{role}: {contents}");
        }
        if role == "missing" {
            assert_eq!(
                contents.matches("[info] CLI upgraded").count(),
                1,
                "{contents}"
            );
        }
        if role == "parent-output" {
            let output = std::fs::read_to_string(fixture.path().join("child-output")).unwrap();
            assert!(
                output.starts_with(
                    "Update · fixture\n\nCLI upgraded · old → new\n\nNo applicable tasks · 1m "
                ),
                "{output}"
            );
            assert_eq!(output.matches("CLI upgraded").count(), 1, "{output}");
            assert_eq!(
                contents.matches("[info] CLI upgraded").count(),
                1,
                "{contents}"
            );
            let tail = std::fs::read_to_string(fixture.path().join("child-output-tail")).unwrap();
            assert!(tail.starts_with("\nNo applicable tasks · 1m "), "{tail}");
            assert!(!tail.contains("CLI upgraded"), "{tail}");
        }
        if role == "parent-interrupt" {
            assert!(!contents.contains("interrupt received"), "{contents}");
            assert!(!contents.contains("force quit"), "{contents}");
            assert_eq!(
                std::fs::read_to_string(fixture.path().join("child-completed-work")).unwrap(),
                "preserved"
            );
        }
    }
}
