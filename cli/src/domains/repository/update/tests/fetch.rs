use super::*;

#[test]
fn failed_fetch_does_not_authorize_using_configuration_edited_while_offline() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let root = fixture.path();
    let config_file = root.join("tracked-config");
    std::fs::write(&config_file, "original").unwrap();
    let tracked = config_file.clone();
    let mut mock = MockExecutor::new();
    mock.expect_execute().returning(move |spec| {
        let args = spec.arguments();
        let output = if args == ["symbolic-ref", "--quiet", "HEAD"] {
            "refs/heads/main".to_string()
        } else if args == TRACKED_STATUS {
            if std::fs::read_to_string(&tracked).unwrap() == "edited" {
                " M tracked-config".to_string()
            } else {
                String::new()
            }
        } else if args == REMOTE_MAIN {
            "origin".to_string()
        } else if args == MERGE_MAIN {
            "refs/heads/main".to_string()
        } else if args == HEAD_SNAPSHOT {
            snapshot(MAIN_NEW)
        } else if args == FETCH_MAIN {
            std::fs::write(&tracked, "edited").unwrap();
            return Err(git_non_zero("remote access denied"));
        } else {
            panic!("unexpected command: {args:?}");
        };
        Ok(ExecResult::success(output))
    });
    let ctx = make_update_context(root, None, mock);
    let (result, signal) = run_update(&ctx);
    assert!(matches!(result.unwrap(), TaskResult::Failed(_)));
    assert!(!signal.was_updated());
    assert!(
        signal.recovery_required(),
        "stale configuration must stop the run"
    );

    // A new invocation sees the user's edit and does not fetch or merge over it.
    let retry = UpdateRepository::new(UpdateSignal::new())
        .run(&ctx)
        .unwrap();
    assert!(
        matches!(retry, TaskResult::Skipped { ref reason, .. } if reason.contains("local changes present"))
    );
    assert_eq!(std::fs::read_to_string(config_file).unwrap(), "edited");
}

#[test]
fn failed_fetch_only_allows_independent_tasks_when_checkout_can_be_verified() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let root = fixture.path();
    for readable in [true, false] {
        let exec = GitScript::new()
            .before_fetch(root, MAIN_NEW)
            .reply(root, FETCH_MAIN, Err(git_error("remote access denied")))
            .git(root, HEAD_SNAPSHOT, snapshot(MAIN_NEW));
        let exec = if readable {
            exec.git(root, TRACKED_STATUS, "")
        } else {
            exec.reply(root, TRACKED_STATUS, Err(git_error("cannot read status")))
        };
        let (result, signal) = exec.run(root, None);
        if readable {
            assert!(matches!(result, Ok(TaskResult::Failed(s)) if s == "git fetch failed"));
        } else {
            let err = result.unwrap_err();
            assert!(format!("{err:#}").contains("cannot read status"));
            assert!(err.downcast_ref::<ExecError>().is_some());
        }
        assert_eq!(signal.recovery_required(), !readable);
        assert!(!signal.was_updated());
    }
}

#[test]
fn run_propagates_fetch_cancellation() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let exec = ScriptedExecutor::new()
        .ok("refs/heads/main")
        .git(root, TRACKED_STATUS, "")
        .git(root, REMOTE_MAIN, "origin")
        .git(root, MERGE_MAIN, "refs/heads/main")
        .git(root, HEAD_SNAPSHOT, snapshot(MAIN_NEW))
        .command(
            CommandSpec::new("git").args(FETCH_MAIN).current_dir(root),
            Err(cancelled_git_error()),
        );
    let ctx = make_update_context(root, None, exec);

    let err = UpdateRepository::new(UpdateSignal::new())
        .run(&ctx)
        .expect_err("fetch cancellation must escape task failure accounting");

    assert!(
        err.downcast_ref::<ExecError>()
            .is_some_and(ExecError::is_cancelled)
    );
}

#[test]
fn run_does_not_retry_cancelled_fetch_with_transient_output() {
    let root = tempfile::tempdir_in(".").unwrap();
    let root = root.path();
    let exec = GitScript::new().before_fetch(root, "old").reply(
        root,
        FETCH_MAIN,
        Err(ExecError::Cancelled {
            command: "git".into(),
            result: ExecResult::failure("", "Connection reset by peer", None),
        }),
    );
    let (result, signal) = exec.run(root, None);
    let error =
        result.expect_err("cancellation takes precedence over transient stderr and must not retry");

    assert!(
        error
            .downcast_ref::<ExecError>()
            .is_some_and(ExecError::is_cancelled),
        "{error:#}"
    );
    assert!(!signal.was_updated());
}

#[test]
fn run_retries_transient_fetch_failure() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let exec = GitScript::new()
        .before_fetch(root, MAIN_NEW)
        .reply(
            root,
            FETCH_MAIN,
            Err(git_error(
                "mux_client_request_session: read from master failed: Connection reset by peer\n\
             Failed to connect to new control master",
            )),
        )
        .git(root, FETCH_MAIN, fetch_output(MAIN_NEW))
        .fetched_object(root, MAIN_NEW, MAIN_NEW)
        .checkout_status(root, MAIN_NEW, "");
    let (result, repo_updated) = exec.run(root, None);

    assert!(matches!(result.unwrap(), TaskResult::Ok));
    assert!(!repo_updated.was_updated());
}

#[test]
fn run_stops_after_transient_fetch_retries_are_exhausted() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let mut exec = GitScript::new().before_fetch(root, MAIN_NEW);
    for _ in 0..3 {
        exec = exec.reply(root, FETCH_MAIN, Err(git_error("connection reset by peer")));
    }
    let exec = exec.checkout_status(root, MAIN_NEW, "");
    let (result, _) = exec.run(root, None);

    assert!(matches!(result.unwrap(), TaskResult::Failed(ref s) if s.contains("git fetch failed")));
}
