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

#[test]
fn fetch_all_runs_inline_when_sequential_or_single_and_fetched_commit_and_recovery_edge_cases() {
    use super::apply::{fetch_all, fetched_commit};
    use super::models::{CheckedRepository, UpdateTarget, UpdateTargetKind};
    use super::recovery::checkout_revision;

    // fetched_commit: handles pruned '-' lines, rejects wrong length, non-hex, and mismatched OIDs.
    let oid_a = "a".repeat(40);
    let oid_b = "b".repeat(40);
    let pruned =
        format!("- {oid_a} {oid_b} refs/remotes/origin/old\n  {oid_a} {oid_b} refs/heads/main\n");
    assert_eq!(fetched_commit(&pruned), Some(oid_b.as_str()));
    assert_eq!(
        fetched_commit(&format!("  {oid_a} {} refs/heads/main\n", "a".repeat(20))),
        None
    );
    assert_eq!(
        fetched_commit(&format!("  {oid_a} {} refs/heads/main\n", "g".repeat(40))),
        None
    );
    assert_eq!(
        fetched_commit(&format!(
            "  {oid_a} {oid_a} refs/heads/main\n  {oid_a} {oid_b} refs/heads/main\n"
        )),
        None
    );

    // fetch_all runs inline on the caller thread for (parallel=true, len=1) and (parallel=false, len=2).
    let caller = std::thread::current().id();
    let dir = tempfile::tempdir().unwrap();
    let repo_a = dir.path().join("a");
    let repo_b = dir.path().join("b");
    std::fs::create_dir(&repo_a).unwrap();
    std::fs::create_dir(&repo_b).unwrap();
    let make_checked = |kind, path: PathBuf| CheckedRepository {
        target: UpdateTarget::new(kind, path),
        head_ref: "refs/heads/main".into(),
    };
    let fetch_out = format!("  {oid_a} {oid_b} refs/heads/main\n");

    let mut mock_single = MockExecutor::new();
    let out1 = fetch_out.clone();
    mock_single.expect_execute().once().returning(move |_| {
        assert_eq!(std::thread::current().id(), caller);
        Ok(ExecResult::success(&out1))
    });
    let ctx_single = make_context(
        repo_a.clone(),
        None,
        Platform::new(Os::Linux, false),
        Arc::new(mock_single),
    )
    .with_parallel(true);
    let checked_a = make_checked(UpdateTargetKind::Main, repo_a.clone());
    let checked_b = make_checked(UpdateTargetKind::Overlay, repo_b.clone());
    let commits = fetch_all(
        &ctx_single,
        &[(&checked_a, "origin".into(), "refs/heads/main".into())],
    )
    .unwrap()
    .unwrap();
    assert_eq!(commits, vec![oid_b.clone()]);

    let mut mock_seq = MockExecutor::new();
    let out2 = fetch_out;
    mock_seq.expect_execute().times(2).returning(move |_| {
        assert_eq!(std::thread::current().id(), caller);
        Ok(ExecResult::success(&out2))
    });
    let ctx_seq = make_context(
        repo_a,
        Some(repo_b),
        Platform::new(Os::Linux, false),
        Arc::new(mock_seq),
    )
    .with_parallel(false);
    let seq_commits = fetch_all(
        &ctx_seq,
        &[
            (&checked_a, "origin".into(), "refs/heads/main".into()),
            (&checked_b, "origin".into(), "refs/heads/main".into()),
        ],
    )
    .unwrap()
    .unwrap();
    assert_eq!(seq_commits, vec![oid_b.clone(), oid_b]);

    // checkout_revision propagates non-UnbornBranch git2 errors rather than treating them as unborn.
    let broken = tempfile::tempdir().unwrap();
    git2::Repository::init(broken.path()).unwrap();
    std::fs::write(
        broken.path().join(".git/HEAD"),
        "ref: refs/heads/bad..name\n",
    )
    .unwrap();
    let err = checkout_revision(broken.path()).unwrap_err();
    assert!(
        !format!("{err:#}").contains("unborn Git HEAD"),
        "non-unborn error was misclassified: {err:#}"
    );
}

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

mod checkout;
mod fetch;
mod multi_repository;

#[test]
fn apply_repository_updates_handles_first_merge_failure_and_inspect_errors() {
    // 1) First repo fails merge --ff-only with changed.is_empty(): must not say "after updating".
    let dir1 = tempfile::tempdir().unwrap();
    let main1 = dir1.path().join("main");
    let exec_first_fail = GitScript::new()
        .before_fetch(&main1, "aaa111")
        .git(&main1, FETCH_MAIN, fetch_output(MAIN_NEW))
        .fetched_object(&main1, "aaa111", MAIN_NEW)
        .ancestor(&main1, "aaa111", MAIN_NEW, true)
        .checkout_status(&main1, "aaa111", "")
        .reply(
            &main1,
            &merge_args(MAIN_NEW),
            Err(git_non_zero("merge conflict")),
        )
        .checkout_status(&main1, "aaa111", "");
    let (first_res, _) = exec_first_fail.run(&main1, None);
    let TaskResult::Failed(msg) = first_res.unwrap() else {
        panic!("expected Failed for first-repo merge conflict");
    };
    assert!(
        !msg.contains("after updating"),
        "single-repo failure must not use partial_merge_reason: {msg}"
    );

    // 2) First repo fails merge --ff-only and subsequent checkout_head fails while NOT cancelled:
    // returns Ok(TaskResult::Failed(... "the checkout state could not be verified")).
    let dir2 = tempfile::tempdir().unwrap();
    let main2 = dir2.path().join("main");
    let exec_inspect_fail = GitScript::new()
        .before_fetch(&main2, "aaa111")
        .git(&main2, FETCH_MAIN, fetch_output(MAIN_NEW))
        .fetched_object(&main2, "aaa111", MAIN_NEW)
        .ancestor(&main2, "aaa111", MAIN_NEW, true)
        .checkout_status(&main2, "aaa111", "")
        .reply(
            &main2,
            &merge_args(MAIN_NEW),
            Err(git_non_zero("merge failed")),
        )
        .reply(&main2, HEAD_SNAPSHOT, Err(git_non_zero("rev-parse failed")));
    let (inspect_res, _) = exec_inspect_fail.run(&main2, None);
    let TaskResult::Failed(inspect_msg) = inspect_res.unwrap() else {
        panic!("expected Failed when checkout_head fails after merge");
    };
    assert!(
        inspect_msg.contains("the checkout state could not be verified"),
        "{inspect_msg}"
    );

    // 3) Same inspect failure when ctx IS cancelled during checkout_head -> returns Err.
    let dir3 = tempfile::tempdir().unwrap();
    let main3 = dir3.path().join("main");
    let cancel_token = crate::infra::cancellation::CancellationToken::new();
    let cancel_in_exec = cancel_token.clone();
    let script_cancel = GitScript::new()
        .before_fetch(&main3, "aaa111")
        .git(&main3, FETCH_MAIN, fetch_output(MAIN_NEW))
        .fetched_object(&main3, "aaa111", MAIN_NEW)
        .ancestor(&main3, "aaa111", MAIN_NEW, true)
        .checkout_status(&main3, "aaa111", "")
        .reply(
            &main3,
            &merge_args(MAIN_NEW),
            Err(git_non_zero("merge failed")),
        );
    let mut mock_cancel = MockExecutor::new();
    let seen_merge = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen_merge_c = Arc::clone(&seen_merge);
    mock_cancel.expect_execute().returning(move |spec| {
        if spec.arguments() == merge_args(MAIN_NEW) {
            seen_merge_c.store(true, std::sync::atomic::Ordering::SeqCst);
            return script_cancel.0.execute(spec);
        }
        if seen_merge_c.load(std::sync::atomic::Ordering::SeqCst)
            && spec.arguments() == HEAD_SNAPSHOT
        {
            cancel_in_exec.cancel();
            return Err(git_non_zero("rev-parse cancelled"));
        }
        script_cancel.0.execute(spec)
    });
    let ctx_cancel = make_update_context(&main3, None, mock_cancel).with_cancellation(cancel_token);
    let (res_cancel, _) = run_update(&ctx_cancel);
    assert!(
        res_cancel.is_err(),
        "expected Err when cancelled during inspect"
    );
}

#[test]
fn apply_repository_updates_partial_report_distinguishes_cancel_from_failure() {
    use crate::engine::BatchReport;

    // Partial error after main updated: non-cancelled has failed=1, cancelled has failed=0.
    for (cancelled, expected_failed) in [(false, 1u32), (true, 0u32)] {
        let dir4 = tempfile::tempdir().unwrap();
        let main4 = dir4.path().join("main");
        let overlay4 = dir4.path().join("overlay");
        let overlay_err = if cancelled {
            ExecError::Cancelled {
                command: "git".into(),
                result: ExecResult::failure("", "cancelled", None),
            }
        } else {
            git_non_zero("merged_head failed")
        };
        let exec_partial = GitScript::new()
            .branch_status(&main4, "refs/heads/main", "")
            .branch_status(&overlay4, "refs/heads/main", "")
            .tracking(&main4)
            .tracking(&overlay4)
            .git(&main4, HEAD_SNAPSHOT, snapshot("aaa111"))
            .git(&overlay4, HEAD_SNAPSHOT, snapshot("def456"))
            .git(&main4, FETCH_MAIN, fetch_output(MAIN_NEW))
            .git(&overlay4, FETCH_MAIN, fetch_output(OVERLAY_NEW))
            .fetched_object(&main4, "aaa111", MAIN_NEW)
            .ancestor(&main4, "aaa111", MAIN_NEW, true)
            .fetched_object(&overlay4, "def456", OVERLAY_NEW)
            .ancestor(&overlay4, "def456", OVERLAY_NEW, true)
            .checkout_status(&main4, "aaa111", "")
            .git(&main4, &merge_args(MAIN_NEW), "Fast-forward")
            .merged_head(&main4, MAIN_NEW)
            .checkout_status(&overlay4, "def456", "")
            .git(&overlay4, &merge_args(OVERLAY_NEW), "Fast-forward")
            .reply(&overlay4, HEAD_SNAPSHOT, Err(overlay_err));
        let (res_partial, _) = exec_partial.run(&main4, Some(&overlay4));
        let err = res_partial.unwrap_err();
        let report = err
            .downcast_ref::<BatchReport>()
            .expect("partial error should attach BatchReport");
        assert_eq!(report.stats().changed_count(), 1);
        assert_eq!(
            report.stats().failed_count(),
            expected_failed,
            "cancelled={cancelled}"
        );
    }
}
