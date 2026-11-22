//! Unit tests for the repository update task.
use super::RepositoryUpdateSignal as UpdateSignal;
use super::*;
use crate::infra::exec::{CommandSpec, ExecError, ExecResult, Executor, MockExecutor};
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::{
    CapturingOutput, ScriptedExecutor, empty_config, make_context, make_linux_context,
};
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

// -----------------------------------------------------------------------
// run()
// -----------------------------------------------------------------------

fn make_update_context(
    root: &Path,
    overlay: Option<&Path>,
    executor: impl Executor + 'static,
) -> Context {
    git2::Repository::init(root).unwrap();
    if let Some(path) = overlay {
        git2::Repository::init(path).unwrap();
    }
    make_context(
        root.into(),
        overlay.map(Path::to_path_buf),
        Platform::new(Os::Linux, false),
        Arc::new(executor),
    )
}

fn run_update(ctx: &Context) -> (Result<TaskResult>, UpdateSignal) {
    let signal = UpdateSignal::new();
    (UpdateRepository::new(signal.clone()).run(ctx), signal)
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

const FETCH_MAIN: &[&str] = &[
    "fetch",
    "--porcelain",
    "--verbose",
    "--no-tags",
    "--no-recurse-submodules",
    "--",
    "origin",
    "refs/heads/main",
];
const HEAD_SNAPSHOT: &[&str] = &["rev-parse", "HEAD", "--symbolic-full-name", "HEAD"];
const SYMBOLIC_HEAD: &[&str] = &["symbolic-ref", "--quiet", "HEAD"];
const TRACKED_STATUS: &[&str] = &["status", "--porcelain", "--untracked-files=no"];
const UNMERGED_INDEX: &[&str] = &["ls-files", "--unmerged"];
const REMOTE_MAIN: &[&str] = &["config", "--get", "branch.main.remote"];
const MERGE_MAIN: &[&str] = &["config", "--get", "branch.main.merge"];
const MAIN_NEW: &str = fixture_oid(&[b'1'; 40]);
const OVERLAY_NEW: &str = fixture_oid(&[b'2'; 40]);

const fn merge_args(commit: &str) -> [&str; 6] {
    [
        "merge",
        "--ff-only",
        "--no-squash",
        "--no-autostash",
        "--no-overwrite-ignore",
        commit,
    ]
}

const fn fixture_oid(hex: &'static [u8; 40]) -> &'static str {
    match std::str::from_utf8(hex) {
        Ok(oid) => oid,
        Err(_) => panic!("fixture OID must be ASCII"),
    }
}

fn fetch_output(commit: &str) -> String {
    format!("* {} {commit} FETCH_HEAD\n", "0".repeat(40))
}

fn verify_fetched(commit: &str) -> String {
    format!("{commit}^{{commit}}")
}

fn snapshot(oid: &str) -> String {
    format!("{oid}\nrefs/heads/main\n")
}

struct GitScript(ScriptedExecutor);

impl GitScript {
    fn new() -> Self {
        Self(ScriptedExecutor::new())
    }

    fn reply(self, root: &Path, args: &[&str], result: Result<ExecResult, ExecError>) -> Self {
        Self(
            self.0
                .command(CommandSpec::new("git").args(args).current_dir(root), result),
        )
    }

    fn git(self, root: &Path, args: &[&str], stdout: impl Into<String>) -> Self {
        Self(self.0.git(root, args, stdout))
    }

    fn context(self, root: &Path, overlay: Option<&Path>) -> Context {
        let mut mock = MockExecutor::new();
        mock.expect_execute().returning(move |spec| {
            assert!(
                spec.environment()
                    .iter()
                    .all(|(key, _)| key != "GIT_CONFIG_NOSYSTEM"),
                "{spec:?}"
            );
            self.0.execute(spec)
        });
        make_update_context(root, overlay, mock)
    }

    fn run(self, root: &Path, overlay: Option<&Path>) -> (Result<TaskResult>, UpdateSignal) {
        run_update(&self.context(root, overlay))
    }

    fn branch_status(self, root: &Path, branch: &str, status: &str) -> Self {
        self.git(root, SYMBOLIC_HEAD, branch)
            .git(root, TRACKED_STATUS, status)
    }

    fn tracking(self, root: &Path) -> Self {
        self.git(root, REMOTE_MAIN, "origin")
            .git(root, MERGE_MAIN, "refs/heads/main")
    }

    fn before_fetch(self, root: &Path, head: &str) -> Self {
        self.branch_status(root, "refs/heads/main", "")
            .tracking(root)
            .git(root, HEAD_SNAPSHOT, snapshot(head))
    }

    fn fetched_object(self, root: &Path, head: &str, fetched: &str) -> Self {
        self.git(root, HEAD_SNAPSHOT, snapshot(head)).git(
            root,
            &["rev-parse", "--verify", &verify_fetched(fetched)],
            fetched,
        )
    }

    fn ancestor(self, root: &Path, older: &str, newer: &str, is_ancestor: bool) -> Self {
        let result = if is_ancestor {
            Ok(ExecResult::success(""))
        } else {
            Err(git_non_zero(""))
        };
        self.reply(root, &["merge-base", "--is-ancestor", older, newer], result)
    }

    fn checkout_status(self, root: &Path, head: &str, status: &str) -> Self {
        self.git(root, HEAD_SNAPSHOT, snapshot(head))
            .git(root, TRACKED_STATUS, status)
    }

    fn merged_head(self, root: &Path, head: &str) -> Self {
        self.git(root, HEAD_SNAPSHOT, snapshot(head))
            .git(root, UNMERGED_INDEX, "")
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

// -----------------------------------------------------------------------
// run() — parallel fetch
// -----------------------------------------------------------------------

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

// -----------------------------------------------------------------------
// run() — dry-run comparison paths
// -----------------------------------------------------------------------

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
