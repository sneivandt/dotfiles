use super::*;

#[test]
fn should_run_false_when_git_dir_missing() {
    let ctx = make_linux_context(PathBuf::from("/nonexistent/repo"), None);
    let task = UpdateRepository::new(UpdateSignal::new());
    assert!(!task.should_run(&ctx));
}

#[test]
fn should_run_true_when_git_dir_exists() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();

    let ctx = make_linux_context(dir.path().to_path_buf(), None);
    let task = UpdateRepository::new(UpdateSignal::new());
    assert!(task.should_run(&ctx));
}

/// In a git worktree the repo root contains a `.git` *file* (not a
/// directory) that stores the path to the per-worktree git data.
/// `should_run` must return `true` in this layout.
#[test]
fn should_run_true_when_git_is_a_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".git"), "gitdir: ../.git/worktrees/my-wt\n").unwrap();

    let ctx = make_linux_context(dir.path().to_path_buf(), None);
    let task = UpdateRepository::new(UpdateSignal::new());
    assert!(task.should_run(&ctx));
}

#[test]
fn run_returns_not_applicable_when_detached_head() {
    let root = tempfile::tempdir().unwrap();
    let exec = GitScript::new().reply(
        root.path(),
        SYMBOLIC_HEAD,
        Err(git_non_zero("simulated failure")),
    );
    let (result, repo_updated) = exec.run(root.path(), None);
    assert!(
        matches!(
            result.unwrap(),
            TaskResult::NotApplicable(ref reason) if reason.contains("detached HEAD")
        ),
        "a detached checkout cannot update itself but must not block dependent install tasks"
    );
    assert!(!repo_updated.was_updated());
}

#[test]
fn run_propagates_symbolic_ref_operational_failure() {
    let root = tempfile::tempdir().unwrap();
    let exec = ScriptedExecutor::new().err(git_error("git could not start"));
    let ctx = make_update_context(root.path(), None, exec);

    let err = UpdateRepository::new(UpdateSignal::new())
        .run(&ctx)
        .expect_err("spawn failure must not be reported as detached HEAD");

    assert!(
        err.downcast_ref::<ExecError>()
            .is_some_and(|error| matches!(error, ExecError::Spawn { .. }))
    );
}

#[test]
fn symbolic_ref_fatal_exit_is_not_detached_in_apply_or_preview() {
    for dry_run in [false, true] {
        for code in [Some(128), None] {
            let root = tempfile::tempdir_in(".").unwrap();
            let exec = GitScript::new().reply(
                root.path(),
                SYMBOLIC_HEAD,
                Err(ExecError::non_zero(
                    "git",
                    ExecResult::failure("", "fatal: could not read repository configuration", code),
                )),
            );
            let ctx = exec.context(root.path(), None).with_dry_run(dry_run);
            let (result, signal) = run_update(&ctx);
            let error =
                result.expect_err("only exit 1 means detached HEAD; fatal failures must propagate");

            assert!(
                matches!(
                    error.downcast_ref::<ExecError>(),
                    Some(ExecError::NonZero { result: output, .. }) if output.code == code
                ),
                "preserve the failure classification for dry_run={dry_run}: {error:#}"
            );
            assert!(
                !signal.was_updated(),
                "a failed branch probe cannot change the checkout"
            );
        }
    }
}

#[test]
fn run_classifies_repository_state_from_git_output() {
    for (case, head, ancestry, expected_reason) in [
        ("current", MAIN_NEW, None, None),
        ("behind", "old", Some((true, false)), None),
        (
            "ahead",
            "old",
            Some((false, true)),
            Some("local branch is ahead"),
        ),
        (
            "diverged",
            "old",
            Some((false, false)),
            Some("local branch diverged"),
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let script = GitScript::new()
            .before_fetch(root, head)
            .git(root, FETCH_MAIN, fetch_output(MAIN_NEW))
            .fetched_object(root, head, MAIN_NEW);
        let script = match ancestry {
            Some((true, _)) => script
                .ancestor(root, head, MAIN_NEW, true)
                .checkout_status(root, head, "")
                .git(root, &merge_args(MAIN_NEW), "")
                .merged_head(root, MAIN_NEW),
            Some((false, ahead)) => script
                .ancestor(root, head, MAIN_NEW, false)
                .ancestor(root, MAIN_NEW, head, ahead)
                .checkout_status(root, head, ""),
            None => script.checkout_status(root, head, ""),
        };
        let changed = matches!(ancestry, Some((true, _)));
        let (result, signal) = script.run(root, None);
        let result = result.unwrap();
        if let Some(fragment) = expected_reason {
            assert!(
                matches!(result, TaskResult::Skipped { ref reason, .. } if reason.contains(fragment)),
                "{case}: {result:?}"
            );
        } else if changed {
            crate::test_helpers::assert_task_changed(&result);
        } else {
            crate::test_helpers::assert_task_ok(&result);
        }
        assert_eq!(signal.was_updated(), changed, "{case}");
    }
}

#[test]
fn a_changed_checkout_never_reaches_the_merge() {
    for change in [
        "old\nrefs/heads/work\n",
        "old\nHEAD\n",
        "changed\nrefs/heads/main\n",
    ] {
        for before_merge in [false, true] {
            let fixture = tempfile::tempdir().unwrap();
            let root = fixture.path();
            let mut executor = GitScript::new().before_fetch(root, "old").git(
                root,
                FETCH_MAIN,
                fetch_output(MAIN_NEW),
            );
            if before_merge {
                executor = executor
                    .fetched_object(root, "old", MAIN_NEW)
                    .ancestor(root, "old", MAIN_NEW, true);
            }
            let executor = executor.git(root, HEAD_SNAPSHOT, change);
            let (result, signal) = executor.run(root, None);
            assert!(
                matches!(result.unwrap(), TaskResult::Failed(reason) if reason.contains("checkout branch, commit, or tracked files changed")),
                "{change:?}, before_merge={before_merge}",
            );
            assert!(!signal.was_updated());
            assert!(
                signal.recovery_required(),
                "stale inputs must stop the remaining tasks"
            );
        }
    }
}

#[test]
fn current_upstream_does_not_authorize_continuing_with_changed_or_unreadable_inputs() {
    for status in [
        Ok(ExecResult::success(" M conf/git-config.toml")),
        Err(git_error("status failed")),
    ] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        let executor = GitScript::new()
            .before_fetch(root, MAIN_NEW)
            .git(root, FETCH_MAIN, fetch_output(MAIN_NEW))
            .fetched_object(root, MAIN_NEW, MAIN_NEW)
            .git(root, HEAD_SNAPSHOT, snapshot(MAIN_NEW))
            .reply(root, TRACKED_STATUS, status);
        let (result, signal) = executor.run(root, None);
        assert!(matches!(result, Ok(TaskResult::Failed(_)) | Err(_)));
        assert!(signal.recovery_required());
        assert!(!signal.was_updated());
    }
}

#[test]
fn fetch_output_without_a_unique_object_blocks_the_merge() {
    for output in [
        String::new(),
        fetch_output("not-an-object-id"),
        format!("{}{}", fetch_output(MAIN_NEW), fetch_output(OVERLAY_NEW)),
    ] {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let executor = GitScript::new()
            .before_fetch(root, "old")
            .git(root, FETCH_MAIN, output)
            .checkout_status(root, "old", "");
        let (result, signal) = executor.run(root, None);
        assert!(
            matches!(result.unwrap(), TaskResult::Failed(reason) if reason == "git fetch failed")
        );
        assert!(!signal.was_updated());
    }
}

#[test]
fn worktree_has_local_changes_ignores_untracked_files() {
    let root = tempfile::tempdir_in(".").unwrap();

    let executor = ScriptedExecutor::new().git(root.path(), TRACKED_STATUS, "");
    let ctx = make_context(
        root.path().to_path_buf(),
        None,
        Platform::new(Os::Linux, false),
        Arc::new(executor),
    );

    assert!(!worktree_has_local_changes(&ctx, root.path()).unwrap());
}

#[test]
fn apply_requires_both_configured_upstream_values() {
    for (remote, reference) in [("", "refs/heads/main"), ("origin", "")] {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let executor = GitScript::new()
            .branch_status(root, "refs/heads/main", "")
            .git(root, REMOTE_MAIN, remote)
            .git(root, MERGE_MAIN, reference);
        let (result, signal) = executor.run(root, None);
        assert!(
            matches!(result.unwrap(), TaskResult::Skipped { reason, .. } if reason.contains("no upstream tracking branch"))
        );
        assert!(!signal.was_updated());
    }
}

#[test]
fn repeat_update_after_fast_forward_does_not_merge_or_request_another_restart() {
    let root = tempfile::tempdir_in(".").unwrap();
    let root = root.path();
    let mut executor = GitScript::new();
    for head in ["old", MAIN_NEW] {
        executor = executor
            .before_fetch(root, head)
            .git(root, FETCH_MAIN, fetch_output(MAIN_NEW))
            .fetched_object(root, head, MAIN_NEW);
        if head == "old" {
            executor = executor
                .ancestor(root, "old", MAIN_NEW, true)
                .checkout_status(root, head, "")
                .git(root, &merge_args(MAIN_NEW), "updated")
                .merged_head(root, MAIN_NEW);
        } else {
            executor = executor.checkout_status(root, head, "");
        }
    }
    let ctx = executor.context(root, None);

    for first_run in [true, false] {
        let output = Arc::new(CapturingOutput::default());
        let ctx = ctx.with_log(Arc::<CapturingOutput>::clone(&output));
        let (result, signal) = run_update(&ctx);
        let result = result.unwrap();
        if first_run {
            assert_eq!(crate::test_helpers::task_batch(&result).changed_count(), 1);
        } else {
            crate::test_helpers::assert_task_ok(&result);
        }
        assert_eq!(signal.was_updated(), first_run);
        let actions = output.actions();
        assert_eq!(actions.len(), usize::from(first_run));
        if first_run {
            assert_eq!(actions[0].verb, "update");
            assert_eq!(actions[0].subject, "repository");
            assert!(!actions[0].planned);
            assert_eq!(actions[0].message, "update repository");
        }
    }
}

#[test]
fn dry_run_queries_remote_without_fetch_merge_or_restart() {
    for (remote_sha, changed) in [("abc123", false), ("def456", true)] {
        let root = tempfile::tempdir_in(".").unwrap();
        let root = root.path();
        let exec = GitScript::new()
            .branch_status(root, "refs/heads/feature/test", "")
            .git(root, &["rev-parse", "HEAD"], "abc123\n")
            .git(
                root,
                &["config", "--get", "branch.feature/test.remote"],
                "origin",
            )
            .git(
                root,
                &["config", "--get", "branch.feature/test.merge"],
                "refs/heads/feature/test",
            )
            .git(
                root,
                &[
                    "ls-remote",
                    "--exit-code",
                    "--refs",
                    "--",
                    "origin",
                    "refs/heads/feature/test",
                ],
                format!("{remote_sha}\trefs/heads/feature/test\n"),
            );
        let exec = if changed {
            exec.git(
                root,
                &["rev-parse", "--verify", "--quiet", "def456^{commit}"],
                "def456",
            )
            .ancestor(root, "abc123", "def456", true)
        } else {
            exec
        };
        let output = Arc::new(CapturingOutput::default());
        let ctx = exec
            .context(root, None)
            .with_dry_run(true)
            .with_log(Arc::<CapturingOutput>::clone(&output));
        let (result, signal) = run_update(&ctx);
        let result = result.unwrap();

        if changed {
            assert_eq!(crate::test_helpers::task_batch(&result).changed_count(), 1);
        } else {
            crate::test_helpers::assert_task_ok(&result);
        }
        assert!(!signal.was_updated(), "preview must not trigger a restart");
        let actions = output.actions();
        assert_eq!(actions.len(), usize::from(changed));
        if changed {
            assert_eq!(actions[0].verb, "update");
            assert_eq!(actions[0].subject, "repository");
            assert!(actions[0].planned);
            assert_eq!(
                actions[0].message,
                "update repository · fast-forward available (local merge eligibility not checked)"
            );
        }
    }
}

#[test]
fn dry_run_only_treats_documented_probe_exits_as_absence() {
    for (probe, absent_code) in [(0, 1), (1, 1), (2, 2)] {
        for code in [Some(1), Some(2), Some(3), Some(128), None] {
            let root = tempfile::tempdir_in(".").unwrap();
            let root = root.path();
            let mut exec = GitScript::new()
                .branch_status(root, "refs/heads/main", "")
                .git(root, &["rev-parse", "HEAD"], "abc123");
            for (index, (args, stdout)) in [
                (REMOTE_MAIN, "origin"),
                (MERGE_MAIN, "refs/heads/main"),
                (
                    &[
                        "ls-remote",
                        "--exit-code",
                        "--refs",
                        "--",
                        "origin",
                        "refs/heads/main",
                    ],
                    "abc123\trefs/heads/main",
                ),
            ]
            .into_iter()
            .enumerate()
            .take(probe + 1)
            {
                let result = if index == probe {
                    Err(ExecError::non_zero(
                        "git",
                        ExecResult::failure("", "fixture probe failure", code),
                    ))
                } else {
                    Ok(ExecResult::success(stdout))
                };
                exec = exec.reply(root, args, result);
            }
            let ctx = exec.context(root, None).with_dry_run(true);
            let (result, signal) = run_update(&ctx);
            if code == Some(absent_code) {
                let result = result.unwrap();
                assert!(
                    format!("{result:?}").contains("cannot determine fast-forward eligibility")
                );
            } else {
                let error = result.expect_err("failed probes must not fall back to cached refs");
                assert!(
                    matches!(error.downcast_ref::<ExecError>(), Some(ExecError::NonZero { result: output, .. }) if output.code == code),
                    "probe={probe}, code={code:?}: {error:#}"
                );
            }
            assert!(!signal.was_updated(), "preview must not trigger a restart");
        }
    }
}

#[test]
fn only_a_verified_fast_forward_signals_a_successful_update() {
    for (merge, expected_failure) in [
        (
            Err(git_non_zero("merge rejected")),
            Some("git merge --ff-only failed"),
        ),
        (Err(cancelled_git_error()), None),
        (
            Ok(ExecResult::success("")),
            Some("git merge did not leave the expected branch at the fetched commit"),
        ),
    ] {
        let verify = merge.is_ok();
        let merge_failed = merge.as_ref().is_err_and(|error| !error.is_cancelled());
        let root = tempfile::tempdir_in(".").unwrap();
        let root = root.path();
        let mut exec = GitScript::new()
            .before_fetch(root, "old")
            .git(root, FETCH_MAIN, fetch_output(MAIN_NEW))
            .fetched_object(root, "old", MAIN_NEW)
            .ancestor(root, "old", MAIN_NEW, true)
            .checkout_status(root, "old", "")
            .reply(root, &merge_args(MAIN_NEW), merge);
        if verify || merge_failed {
            exec = exec.git(root, HEAD_SNAPSHOT, snapshot("old"));
        }
        if merge_failed {
            exec = exec.git(root, TRACKED_STATUS, "");
        }
        let log = Arc::new(CapturingOutput::default());
        let ctx = exec.context(root, None);
        let ctx = ctx.with_log(Arc::<CapturingOutput>::clone(&log));
        let (result, signal) = run_update(&ctx);
        if let Some(expected) = expected_failure {
            assert!(
                matches!(result.unwrap(), TaskResult::Failed(reason) if reason.contains(expected))
            );
        } else {
            assert!(
                result
                    .unwrap_err()
                    .downcast_ref::<ExecError>()
                    .is_some_and(ExecError::is_cancelled)
            );
        }
        assert!(
            !signal.was_updated(),
            "an unverified merge must not request a restart"
        );
        assert!(
            log.actions().is_empty(),
            "an unverified merge must not report an update"
        );
    }
}

#[test]
fn recovery_check_does_not_discover_a_marked_parent_of_an_export() {
    let parent = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(parent.path()).unwrap();
    std::fs::write(repo.path().join("dotfiles-merge-in-progress"), "").unwrap();
    let export = parent.path().join("export");
    std::fs::create_dir(&export).unwrap();
    ensure_recovery_clear(&export).unwrap();
}
