use super::*;

#[test]
fn run_skips_when_overlay_has_local_changes() {
    let root = tempfile::tempdir().unwrap();
    let main_root = root.path().join("main");
    let overlay = root.path().join("overlay");
    let exec = GitScript::new()
        .branch_status(&main_root, "refs/heads/main", "")
        .branch_status(&overlay, "refs/heads/main", "M  private.toml");
    let (result, repo_updated) = exec.run(&main_root, Some(&overlay));
    assert!(matches!(
        result.unwrap(),
        TaskResult::Skipped { ref reason, .. }
            if reason.contains("local changes") && reason.contains("overlay")
    ));
    assert!(!repo_updated.was_updated());
}

#[test]
fn run_updates_overlay_repository_when_behind_upstream() {
    let root = tempfile::tempdir().unwrap();
    let main_root = root.path().join("main");
    let overlay_root = root.path().join("overlay");
    let exec = fetched_repositories_script(&main_root, MAIN_NEW, &overlay_root, "def456")
        .fetched_object(&main_root, MAIN_NEW, MAIN_NEW)
        .fetched_object(&overlay_root, "def456", OVERLAY_NEW)
        .ancestor(&overlay_root, "def456", OVERLAY_NEW, true)
        .checkout_status(&main_root, MAIN_NEW, "")
        .checkout_status(&overlay_root, "def456", "")
        .git(
            &overlay_root,
            &merge_args(OVERLAY_NEW),
            "Updating def456..fed654\nFast-forward",
        )
        .merged_head(&overlay_root, OVERLAY_NEW);
    let (result, repo_updated) = exec.run(&main_root, Some(&overlay_root));
    crate::test_helpers::assert_task_changed(&result.unwrap());
    assert!(repo_updated.was_updated());
}

fn overlay_config(main: &Path, overlay: &Path) -> crate::Config {
    let mut config = empty_config(main.into());
    config.overlay = Some(overlay.into());
    config
}

fn parallel_update_context(config: &crate::Config, mock: MockExecutor) -> Context {
    make_update_context(&config.root, config.overlay.as_deref(), mock).with_parallel(true)
}

#[test]
fn parallel_fetch_visits_every_repository() {
    let root = tempfile::tempdir().unwrap();
    let main_root = root.path().join("main");
    let overlay = tempfile::tempdir().unwrap();
    let config = overlay_config(&main_root, overlay.path());

    let fetched = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = Arc::clone(&fetched);
    let mut mock = MockExecutor::new();
    mock.expect_execute().returning(move |spec| {
        assert_eq!(spec.program(), "git");
        match spec.arguments().first().and_then(|arg| arg.to_str()) {
            Some("symbolic-ref") => Ok(ExecResult::success("refs/heads/main")),
            Some("status") => Ok(ExecResult::success("")),
            Some("config") if spec.arguments() == REMOTE_MAIN => Ok(ExecResult::success("origin")),
            Some("config") if spec.arguments() == MERGE_MAIN => {
                Ok(ExecResult::success("refs/heads/main"))
            }
            Some("fetch") => {
                recorded
                    .lock()
                    .unwrap()
                    .push(spec.working_dir().unwrap().to_path_buf());
                Ok(ExecResult::success(fetch_output(MAIN_NEW)))
            }
            Some("rev-parse") if spec.arguments() == HEAD_SNAPSHOT => {
                Ok(ExecResult::success(snapshot(MAIN_NEW)))
            }
            Some("rev-parse") => Ok(ExecResult::success(MAIN_NEW)),
            _ => panic!("unexpected git call {:?}", spec.arguments()),
        }
    });

    let ctx = parallel_update_context(&config, mock);
    let repo_updated = UpdateSignal::new();
    let result = UpdateRepository::new(repo_updated).run(&ctx).unwrap();

    assert!(matches!(result, TaskResult::Ok));
    let fetched = fetched.lock().unwrap();
    assert_eq!(fetched.len(), 2, "both repositories should be fetched");
    assert!(fetched.contains(&main_root));
    assert!(fetched.contains(&overlay.path().to_path_buf()));
}

#[test]
fn parallel_fetch_failure_reports_the_first_declared_repository() {
    let root = tempfile::tempdir().unwrap();
    let main_root = root.path().join("main");
    let overlay = tempfile::tempdir().unwrap();
    let config = overlay_config(&main_root, overlay.path());

    let mut mock = MockExecutor::new();
    mock.expect_execute().returning(|spec| {
        assert_eq!(spec.program(), "git");
        match spec.arguments().first().and_then(|arg| arg.to_str()) {
            Some("symbolic-ref") => Ok(ExecResult::success("refs/heads/main")),
            Some("status") => Ok(ExecResult::success("")),
            // Both repositories fail, so the reported reason must come from
            // declaration order rather than whichever thread finished first.
            Some("config") if spec.arguments() == REMOTE_MAIN => Ok(ExecResult::success("origin")),
            Some("config") if spec.arguments() == MERGE_MAIN => {
                Ok(ExecResult::success("refs/heads/main"))
            }
            Some("rev-parse") => Ok(ExecResult::success(snapshot(MAIN_NEW))),
            Some("fetch") => Err(git_error("simulated fetch failure")),
            _ => panic!("unexpected git call {:?}", spec.arguments()),
        }
    });

    let ctx = parallel_update_context(&config, mock);
    let repo_updated = UpdateSignal::new();
    let result = UpdateRepository::new(repo_updated).run(&ctx).unwrap();

    // `UpdateTargetKind::Main` renders the bare reason; the overlay would
    // render "git fetch failed in <overlay>".
    assert!(
        matches!(result, TaskResult::Failed(ref s) if s == "git fetch failed"),
        "expected the main repository's failure reason, got {result:?}"
    );
}

#[test]
fn parallel_fetch_cancellation_takes_precedence_over_another_repository_failure() {
    for cancel_overlay in [false, true] {
        let root = tempfile::tempdir_in(".").unwrap();
        let main_root = root.path().join("main");
        let overlay_root = root.path().join("overlay");
        let config = overlay_config(&main_root, &overlay_root);
        let cancelled_root = if cancel_overlay {
            overlay_root
        } else {
            main_root
        };
        let mut mock = MockExecutor::new();
        mock.expect_execute().times(12).returning(move |spec| {
            assert_eq!(spec.program(), "git");
            assert!(spec.is_checked());
            match spec.arguments().first().and_then(|arg| arg.to_str()) {
                Some("symbolic-ref") => {
                    assert_eq!(spec.arguments(), ["symbolic-ref", "--quiet", "HEAD"]);
                    Ok(ExecResult::success("refs/heads/main"))
                }
                Some("status") => {
                    assert_eq!(
                        spec.arguments(),
                        ["status", "--porcelain", "--untracked-files=no"]
                    );
                    Ok(ExecResult::success(""))
                }
                Some("config") if spec.arguments() == REMOTE_MAIN => {
                    Ok(ExecResult::success("origin"))
                }
                Some("config") if spec.arguments() == MERGE_MAIN => {
                    Ok(ExecResult::success("refs/heads/main"))
                }
                Some("rev-parse") => {
                    assert_eq!(spec.arguments(), HEAD_SNAPSHOT);
                    Ok(ExecResult::success(snapshot(MAIN_NEW)))
                }
                Some("fetch") => {
                    assert_eq!(spec.arguments(), FETCH_MAIN);
                    Err(if spec.working_dir() == Some(cancelled_root.as_path()) {
                        cancelled_git_error()
                    } else {
                        git_error("simulated fetch failure")
                    })
                }
                _ => panic!("unexpected git call {:?}", spec.arguments()),
            }
        });
        let ctx = parallel_update_context(&config, mock);
        let signal = UpdateSignal::new();

        let error = UpdateRepository::new(signal.clone())
            .run(&ctx)
            .expect_err("cancellation must escape even when the other fetch also fails");

        assert!(
            error
                .downcast_ref::<ExecError>()
                .is_some_and(ExecError::is_cancelled),
            "cancel_overlay={cancel_overlay}: {error:#}"
        );
        assert!(!signal.was_updated());
    }
}

#[test]
fn multi_repository_merge_records_verified_advances_and_preserves_restart_signal() {
    for merge in [
        Err(git_error("second merge could not start")),
        Err(git_non_zero("second merge failed")),
        Err(cancelled_git_error()),
        Ok(ExecResult::success("updated")),
    ] {
        let cancelled = merge.as_ref().is_err_and(ExecError::is_cancelled);
        let success = merge.is_ok();
        let root = tempfile::tempdir_in(".").unwrap();
        let main_root = root.path().join("main");
        let overlay_root = root.path().join("overlay");
        let executor =
            fetched_repositories_script(&main_root, "main-old", &overlay_root, "overlay-old")
                .fetched_object(&main_root, "main-old", MAIN_NEW)
                .ancestor(&main_root, "main-old", MAIN_NEW, true)
                .fetched_object(&overlay_root, "overlay-old", OVERLAY_NEW)
                .ancestor(&overlay_root, "overlay-old", OVERLAY_NEW, true)
                .checkout_status(&main_root, "main-old", "")
                .git(&main_root, &merge_args(MAIN_NEW), "updated")
                .merged_head(&main_root, MAIN_NEW)
                .checkout_status(&overlay_root, "overlay-old", "")
                .reply(&overlay_root, &merge_args(OVERLAY_NEW), merge);
        let executor = if success {
            executor.merged_head(&overlay_root, OVERLAY_NEW)
        } else if cancelled {
            executor
        } else {
            executor.checkout_status(&overlay_root, "overlay-old", "")
        };
        let (result, signal) = executor.run(&main_root, Some(&overlay_root));

        if cancelled {
            let error = result.unwrap_err();
            assert!(error.downcast_ref::<ExecError>().unwrap().is_cancelled());
            let report = error.downcast_ref::<crate::engine::BatchReport>().unwrap();
            assert_eq!(report.stats().changed_count(), 1);
            assert_eq!(report.interrupted_count(), 1);
        } else {
            let result = result.unwrap();
            let stats = crate::test_helpers::task_batch(&result);
            assert_eq!(stats.changed_count(), if success { 2 } else { 1 });
            assert_eq!(stats.failed_count(), u32::from(!success));
            if !success {
                let reason = stats.message().unwrap();
                assert!(reason.contains("git merge --ff-only failed in overlay repository"));
                assert!(reason.contains("Already advanced repository"));
            }
        }
        assert_eq!(signal.recovery_required(), !success);
        assert_eq!(
            overlay_root
                .join(".git/dotfiles-merge-in-progress")
                .exists(),
            !success
        );
        assert!(!main_root.join(".git/dotfiles-merge-in-progress").exists());
        assert!(
            signal.was_updated(),
            "a successful first merge must prevent later tasks from using stale configuration"
        );
    }
}

fn fetched_repositories_script(
    main: &Path,
    main_head: &str,
    overlay: &Path,
    overlay_head: &str,
) -> GitScript {
    GitScript::new()
        .branch_status(main, "refs/heads/main", "")
        .branch_status(overlay, "refs/heads/main", "")
        .tracking(main)
        .tracking(overlay)
        .git(main, HEAD_SNAPSHOT, snapshot(main_head))
        .git(overlay, HEAD_SNAPSHOT, snapshot(overlay_head))
        .git(main, FETCH_MAIN, fetch_output(MAIN_NEW))
        .git(overlay, FETCH_MAIN, fetch_output(OVERLAY_NEW))
}

#[test]
fn unsafe_overlay_plan_prevents_merging_an_otherwise_ready_main_repository() {
    for (ahead, reason) in [
        (
            false,
            "local branch diverged from upstream in overlay repository",
        ),
        (
            true,
            "local branch is ahead of upstream in overlay repository",
        ),
    ] {
        let root = tempfile::tempdir_in(".").unwrap();
        let main_root = root.path().join("main");
        let overlay_root = root.path().join("overlay");
        let executor =
            fetched_repositories_script(&main_root, "main-old", &overlay_root, "overlay-old")
                .fetched_object(&main_root, "main-old", MAIN_NEW)
                .ancestor(&main_root, "main-old", MAIN_NEW, true)
                .fetched_object(&overlay_root, "overlay-old", OVERLAY_NEW)
                .ancestor(&overlay_root, "overlay-old", OVERLAY_NEW, false)
                .ancestor(&overlay_root, OVERLAY_NEW, "overlay-old", ahead)
                .checkout_status(&main_root, "main-old", "")
                .checkout_status(&overlay_root, "overlay-old", "");
        let (result, signal) = executor.run(&main_root, Some(&overlay_root));
        let result = result.unwrap();

        assert!(
            matches!(&result, TaskResult::Skipped { reason: detail, kind }
                if kind.is_failure()
                    && detail == reason),
            "{ahead}: unsafe overlay must block the entire update: {result:?}"
        );
        assert!(!signal.was_updated(), "{ahead}: no merge may have occurred");
    }
}

#[test]
fn skipping_ahead_main_still_checks_the_later_overlay_inputs() {
    let root = tempfile::tempdir().unwrap();
    let main = root.path().join("main");
    let overlay = root.path().join("overlay");
    let exec = fetched_repositories_script(&main, "main-old", &overlay, "overlay-old")
        .fetched_object(&main, "main-old", MAIN_NEW)
        .ancestor(&main, "main-old", MAIN_NEW, false)
        .ancestor(&main, MAIN_NEW, "main-old", true)
        .checkout_status(&main, "main-old", "")
        .checkout_status(&overlay, "overlay-old", " M conf/symlinks.toml");
    let (result, signal) = exec.run(&main, Some(&overlay));
    assert!(matches!(result.unwrap(),
        TaskResult::Failed(reason) if reason.contains("in overlay repository")));
    assert!(signal.recovery_required());
    assert!(!signal.was_updated());
}
