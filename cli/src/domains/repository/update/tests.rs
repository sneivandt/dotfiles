//! Unit tests for the repository update task.
use super::RepositoryUpdateSignal as UpdateSignal;
use super::*;
use crate::infra::exec::{CommandSpec, ExecError, ExecResult, Executor, MockExecutor};
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::{ScriptedExecutor, empty_config, make_context, make_linux_context};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn git_error(message: &str) -> ExecError {
    ExecError::spawn("git", std::io::Error::other(message.to_string()))
}

fn git_non_zero(message: &str) -> ExecError {
    ExecError::non_zero("git", ExecResult::failure("", message, Some(1)))
}

fn cancelled_git_error() -> ExecError {
    ExecError::Cancelled {
        command: "git".to_string(),
        result: ExecResult::failure("", "", None),
    }
}

#[test]
fn should_run_false_when_git_dir_missing() {
    let config = empty_config(PathBuf::from("/nonexistent/repo"));
    let ctx = make_linux_context(config);
    let task = UpdateRepository::new(UpdateSignal::new());
    assert!(!task.should_run(&ctx));
}

#[test]
fn should_run_true_when_git_dir_exists() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    let config = empty_config(dir.path().to_path_buf());
    let ctx = make_linux_context(config);
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
    let config = empty_config(dir.path().to_path_buf());
    let ctx = make_linux_context(config);
    let task = UpdateRepository::new(UpdateSignal::new());
    assert!(task.should_run(&ctx));
}

// -----------------------------------------------------------------------
// run()
// -----------------------------------------------------------------------

/// Build a context that uses the given executor double so we can control git
/// responses. Accepts both [`ScriptedExecutor`] and [`MockExecutor`].
fn make_update_context(config: crate::Config, executor: impl Executor + 'static) -> Context {
    make_context(config, Platform::new(Os::Linux, false), Arc::new(executor))
}

#[test]
fn run_returns_not_applicable_when_detached_head() {
    let config = empty_config(PathBuf::from("/tmp"));
    // First call (symbolic-ref): fails → detached HEAD
    let exec = ScriptedExecutor::new().err(git_non_zero("simulated failure"));
    let ctx = make_update_context(config, exec);
    let repo_updated = UpdateSignal::new();
    let task = UpdateRepository::new(repo_updated.clone());

    let result = task.run(&ctx).unwrap();
    assert!(
        matches!(
            result,
            TaskResult::NotApplicable(ref reason) if reason.contains("detached HEAD")
        ),
        "a detached checkout cannot update itself but must not block dependent install tasks"
    );
    assert!(!repo_updated.was_updated());
}

#[test]
fn run_propagates_symbolic_ref_operational_failure() {
    let config = empty_config(PathBuf::from("/tmp"));
    let exec = ScriptedExecutor::new().err(git_error("git could not start"));
    let ctx = make_update_context(config, exec);

    let err = UpdateRepository::new(UpdateSignal::new())
        .run(&ctx)
        .expect_err("spawn failure must not be reported as detached HEAD");

    assert!(
        err.downcast_ref::<ExecError>()
            .is_some_and(|error| matches!(error, ExecError::Spawn { .. }))
    );
}

/// Run [`UpdateRepository`] against a scripted sequence of successful git
/// command outputs, returning the task result and whether the repository was
/// marked as updated.
///
/// The task issues git commands in a fixed order — `symbolic-ref`, `status`,
/// `fetch`, `rev-parse HEAD`, `rev-parse @{u}`, `rev-list --count`, and
/// `merge --ff-only` — so a case only supplies stdout for as many calls as it
/// expects the task to reach.
fn run_with_git_output(outputs: &[&str]) -> (TaskResult, bool) {
    let exec = outputs
        .iter()
        .fold(ScriptedExecutor::new(), |exec, stdout| exec.ok(*stdout));
    let ctx = make_update_context(empty_config(PathBuf::from("/tmp")), exec);
    let signal = UpdateSignal::new();
    let result = UpdateRepository::new(signal.clone()).run(&ctx).unwrap();
    (result, signal.was_updated())
}

/// The task classifies repository state purely from the sequence of git
/// command outputs, so one table covers every classification branch.
///
/// Each case lists the expected outcome as `None` for [`TaskResult::Ok`] or
/// `Some(fragment)` for a skip whose reason contains `fragment`, plus whether
/// the update signal should end up set.
#[test]
fn run_classifies_repository_state_from_git_output() {
    let cases: [(&str, &[&str], Option<&str>, bool); 5] = [
        (
            "staged changes leave the worktree dirty",
            &["refs/heads/main", "M  dirty_file.txt"],
            Some("local changes"),
            false,
        ),
        (
            "HEAD already matches upstream",
            &["refs/heads/main", "", "", "abc123", "abc123"],
            None,
            false,
        ),
        (
            "fast-forward merge brings in new commits",
            &[
                "refs/heads/main",
                "",
                "",
                "abc1234",
                "def5678",
                "0",
                "Updating abc1234..def5678\nFast-forward",
            ],
            None,
            true,
        ),
        (
            "local commits ahead of upstream mean the branch diverged",
            &["refs/heads/main", "", "", "abc1234", "def5678", "2"],
            Some("diverged"),
            false,
        ),
        (
            "rev-list count that is not a number",
            &[
                "refs/heads/main",
                "",
                "",
                "abc1234",
                "def5678",
                "not-a-count",
            ],
            Some("could not determine"),
            false,
        ),
    ];

    for (case, outputs, expected_skip, expect_updated) in cases {
        let (result, updated) = run_with_git_output(outputs);

        if let Some(fragment) = expected_skip {
            assert!(
                matches!(
                    result,
                    TaskResult::Skipped { ref reason, .. } if reason.contains(fragment)
                ),
                "{case}: expected a skip mentioning {fragment:?}, got {result:?}"
            );
        } else {
            assert!(
                matches!(result, TaskResult::Ok),
                "{case}: expected Ok, got {result:?}"
            );
        }
        assert_eq!(updated, expect_updated, "{case}: update signal");
    }
}

#[derive(Debug)]
struct UntrackedAwareExecutor;

impl Executor for UntrackedAwareExecutor {
    fn execute(&self, spec: CommandSpec) -> std::result::Result<ExecResult, ExecError> {
        let stdout = if spec
            .arguments()
            .iter()
            .any(|arg| arg == "--untracked-files=no")
        {
            String::new()
        } else {
            "?? new-file.txt\n".to_string()
        };

        Ok(ExecResult::success(stdout))
    }

    fn which(&self, _: &str) -> bool {
        false
    }

    fn which_path(&self, program: &str) -> Result<PathBuf> {
        anyhow::bail!("{program} not found on PATH")
    }
}

#[test]
fn worktree_has_local_changes_ignores_untracked_files() {
    let config = empty_config(PathBuf::from("/repo"));
    let ctx = make_context(
        config,
        Platform::new(Os::Linux, false),
        Arc::new(UntrackedAwareExecutor),
    );

    assert!(!worktree_has_local_changes(&ctx, Path::new("/repo"), &[]).unwrap());
}

#[test]
fn run_returns_failed_when_fetch_fails() {
    let config = empty_config(PathBuf::from("/tmp"));
    // 1. symbolic-ref → on a branch
    // 2. status → clean worktree
    // 3. fetch → fails
    let exec = ScriptedExecutor::new()
        .ok("refs/heads/main")
        .ok("")
        .err(git_error("simulated fetch failure"));
    let ctx = make_update_context(config, exec);
    let repo_updated = UpdateSignal::new();
    let task = UpdateRepository::new(repo_updated);

    let result = task.run(&ctx).unwrap();
    assert!(matches!(result, TaskResult::Failed(ref s) if s.contains("git fetch failed")));
}

#[test]
fn run_propagates_fetch_cancellation() {
    let config = empty_config(PathBuf::from("/tmp"));
    let exec = ScriptedExecutor::new()
        .ok("refs/heads/main")
        .ok("")
        .err(cancelled_git_error());
    let ctx = make_update_context(config, exec);

    let err = UpdateRepository::new(UpdateSignal::new())
        .run(&ctx)
        .expect_err("fetch cancellation must escape task failure accounting");

    assert!(
        err.downcast_ref::<ExecError>()
            .is_some_and(ExecError::is_cancelled)
    );
}

#[test]
fn run_retries_transient_fetch_failure() {
    let config = empty_config(PathBuf::from("/tmp"));
    let exec = ScriptedExecutor::new()
        .ok("refs/heads/main")
        .ok("")
        .err(git_error(
            "mux_client_request_session: read from master failed: Connection reset by peer\n\
             Failed to connect to new control master",
        ))
        .ok("")
        .ok("abc123")
        .ok("abc123");
    let ctx = make_update_context(config, exec);
    let repo_updated = UpdateSignal::new();
    let task = UpdateRepository::new(repo_updated.clone());

    let result = task.run(&ctx).unwrap();

    assert!(matches!(result, TaskResult::Ok));
    assert!(!repo_updated.was_updated());
}

#[test]
fn run_stops_after_transient_fetch_retries_are_exhausted() {
    let config = empty_config(PathBuf::from("/tmp"));
    let exec = ScriptedExecutor::new()
        .ok("refs/heads/main")
        .ok("")
        .err(git_error("connection reset by peer"))
        .err(git_error("connection reset by peer"))
        .err(git_error("connection reset by peer"));
    let ctx = make_update_context(config, exec);
    let task = UpdateRepository::new(UpdateSignal::new());

    let result = task.run(&ctx).unwrap();

    assert!(matches!(result, TaskResult::Failed(ref s) if s.contains("git fetch failed")));
}

#[test]
fn run_skips_when_overlay_has_local_changes() {
    let main_root = PathBuf::from("/tmp/main");
    let overlay = tempfile::tempdir().unwrap();
    std::fs::create_dir(overlay.path().join(".git")).unwrap();
    let overlay_root = overlay.path().to_path_buf();
    let mut config = empty_config(main_root.clone());
    config.overlay = Some(overlay_root.clone());

    let exec = ScriptedExecutor::new()
        .git(
            main_root.clone(),
            &["symbolic-ref", "--quiet", "HEAD"],
            "refs/heads/main",
        )
        .git(
            main_root,
            &["status", "--porcelain", "--untracked-files=no"],
            "",
        )
        .git(
            overlay_root.clone(),
            &["symbolic-ref", "--quiet", "HEAD"],
            "refs/heads/main",
        )
        .git(
            overlay_root,
            &["status", "--porcelain", "--untracked-files=no"],
            "M  private.toml",
        );

    let ctx = make_update_context(config, exec);
    let repo_updated = UpdateSignal::new();
    let task = UpdateRepository::new(repo_updated.clone());

    let result = task.run(&ctx).unwrap();
    assert!(matches!(
        result,
        TaskResult::Skipped { ref reason, .. }
            if reason.contains("local changes") && reason.contains("overlay")
    ));
    assert!(!repo_updated.was_updated());
}

#[test]
fn run_updates_overlay_repository_when_behind_upstream() {
    let main_root = PathBuf::from("/tmp/main");
    let overlay = tempfile::tempdir().unwrap();
    std::fs::create_dir(overlay.path().join(".git")).unwrap();
    let overlay_root = overlay.path().to_path_buf();
    let mut config = empty_config(main_root.clone());
    config.overlay = Some(overlay_root.clone());

    let exec = ScriptedExecutor::new()
        .git(
            main_root.clone(),
            &["symbolic-ref", "--quiet", "HEAD"],
            "refs/heads/main",
        )
        .git(
            main_root.clone(),
            &["status", "--porcelain", "--untracked-files=no"],
            "",
        )
        .git(
            overlay_root.clone(),
            &["symbolic-ref", "--quiet", "HEAD"],
            "refs/heads/main",
        )
        .git(
            overlay_root.clone(),
            &["status", "--porcelain", "--untracked-files=no"],
            "",
        )
        .git(main_root.clone(), &["fetch", "--quiet"], "")
        .git(overlay_root.clone(), &["fetch", "--quiet"], "")
        .git(main_root.clone(), &["rev-parse", "HEAD"], "abc123")
        .git(main_root, &["rev-parse", "@{u}"], "abc123")
        .git(overlay_root.clone(), &["rev-parse", "HEAD"], "def456")
        .git(overlay_root.clone(), &["rev-parse", "@{u}"], "fed654")
        .git(
            overlay_root.clone(),
            &["rev-list", "--count", "@{u}..HEAD"],
            "0",
        )
        .git(
            overlay_root,
            &["merge", "--ff-only", "@{u}"],
            "Updating def456..fed654\nFast-forward",
        );

    let ctx = make_update_context(config, exec);
    let repo_updated = UpdateSignal::new();
    let task = UpdateRepository::new(repo_updated.clone());

    let result = task.run(&ctx).unwrap();
    assert!(matches!(result, TaskResult::Ok));
    assert!(repo_updated.was_updated());
}

// -----------------------------------------------------------------------
// run() — parallel fetch
// -----------------------------------------------------------------------

/// Build a main + overlay config whose overlay lives in `overlay`.
fn overlay_config(main_root: &Path, overlay: &Path) -> crate::Config {
    std::fs::create_dir_all(overlay.join(".git")).unwrap();
    let mut config = empty_config(main_root.to_path_buf());
    config.overlay = Some(overlay.to_path_buf());
    config
}

#[test]
fn parallel_fetch_visits_every_repository() {
    let main_root = PathBuf::from("/tmp/main");
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
            Some("fetch") => {
                recorded
                    .lock()
                    .unwrap()
                    .push(spec.working_dir().unwrap().to_path_buf());
                Ok(ExecResult::success(""))
            }
            Some("rev-parse") => Ok(ExecResult::success("abc123")),
            _ => panic!("unexpected git call {:?}", spec.arguments()),
        }
    });

    let ctx = make_update_context(config, mock).with_parallel(true);
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
    let main_root = PathBuf::from("/tmp/main");
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
            Some("fetch") => Err(git_error("simulated fetch failure")),
            _ => panic!("unexpected git call {:?}", spec.arguments()),
        }
    });

    let ctx = make_update_context(config, mock).with_parallel(true);
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
fn partial_multi_repository_merge_failure_does_not_request_restart() {
    use super::models::{CheckedRepository, UpdateTarget, UpdateTargetKind};

    let main_root = PathBuf::from("/tmp/main");
    let overlay_root = PathBuf::from("/tmp/overlay");
    let repositories = vec![
        CheckedRepository {
            target: UpdateTarget::new(UpdateTargetKind::Main, main_root),
            head_ref: "refs/heads/main".to_string(),
        },
        CheckedRepository {
            target: UpdateTarget::new(UpdateTargetKind::Overlay, overlay_root),
            head_ref: "refs/heads/main".to_string(),
        },
    ];
    let executor = ScriptedExecutor::new()
        .ok("")
        .ok("")
        .ok("main-old")
        .ok("main-new")
        .ok("0")
        .ok("overlay-old")
        .ok("overlay-new")
        .ok("0")
        .ok("updated")
        .err(git_error("second merge failed"));
    let ctx = make_update_context(empty_config(PathBuf::from("/tmp")), executor);
    let signal = UpdateSignal::new();

    let result = apply_repository_updates(&ctx, &repositories, &[], &signal).unwrap();

    assert!(matches!(result, TaskResult::Failed(_)));
    assert!(
        !signal.was_updated(),
        "a partially updated repository set must not restart into mixed state"
    );
}

// -----------------------------------------------------------------------
// run() — dry-run comparison paths
// -----------------------------------------------------------------------

#[test]
fn run_dry_run_returns_ok_when_already_up_to_date() {
    let config = empty_config(PathBuf::from("/tmp"));
    // symbolic-ref: success → on a branch
    // status --porcelain: empty → clean worktree
    // rev-parse HEAD: abc123
    // branch.main.remote: origin
    // branch.main.merge: refs/heads/main
    // ls-remote origin refs/heads/main: abc123
    let exec = ScriptedExecutor::new()
        .ok("refs/heads/main")
        .ok("")
        .ok("abc123")
        .ok("origin")
        .ok("refs/heads/main")
        .ok("abc123\trefs/heads/main");
    let mut ctx = make_update_context(config, exec);
    ctx = ctx.with_dry_run(true);
    let task = UpdateRepository::new(UpdateSignal::new());

    let result = task.run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok (already up to date in dry-run), got {result:?}"
    );
}

#[test]
fn run_dry_run_returns_dry_run_when_behind_upstream() {
    let config = empty_config(PathBuf::from("/tmp"));
    // symbolic-ref: success
    // status --porcelain: empty
    // rev-parse HEAD: abc123
    // branch.main.remote: origin
    // branch.main.merge: refs/heads/main
    // ls-remote origin refs/heads/main: def456 (different SHA → would pull)
    let exec = ScriptedExecutor::new()
        .ok("refs/heads/main")
        .ok("")
        .ok("abc123")
        .ok("origin")
        .ok("refs/heads/main")
        .ok("def456\trefs/heads/main");
    let mut ctx = make_update_context(config, exec);
    ctx = ctx.with_dry_run(true);
    let task = UpdateRepository::new(UpdateSignal::new());

    let result = task.run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() > 0),
        "expected planned update when behind upstream, got {result:?}"
    );
}

#[test]
fn run_dry_run_returns_ok_when_cached_upstream_matches_head() {
    let config = empty_config(PathBuf::from("/tmp"));
    // symbolic-ref: success
    // status --porcelain: empty
    // rev-parse HEAD: abc123
    // branch.main.remote lookup fails
    // rev-parse @{u}: abc123 (cached tracking ref matches HEAD)
    let exec = ScriptedExecutor::new()
        .ok("refs/heads/main")
        .ok("")
        .ok("abc123")
        .err(git_non_zero("no remote config"))
        .ok("abc123");
    let mut ctx = make_update_context(config, exec);
    ctx = ctx.with_dry_run(true);
    let task = UpdateRepository::new(UpdateSignal::new());

    let result = task.run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok when cached upstream matches HEAD, got {result:?}"
    );
}
