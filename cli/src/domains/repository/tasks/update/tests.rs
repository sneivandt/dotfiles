//! Unit tests for the repository update task.
use super::*;
use crate::engine::UpdateSignal;
use crate::runtime::exec::{ExecResult, Executor, MockExecutor};
use crate::runtime::platform::{Os, Platform};
use crate::test_helpers::{empty_config, make_context, make_linux_context};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn ok_result(stdout: &str) -> ExecResult {
    ExecResult {
        stdout: stdout.to_string(),
        stderr: String::new(),
        success: true,
        code: Some(0),
    }
}

fn expect_git_success(
    mock: &mut MockExecutor,
    seq: &mut mockall::Sequence,
    expected_dir: PathBuf,
    expected_args: &'static [&'static str],
    stdout: &'static str,
) {
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(seq)
        .returning(move |dir, program, args, _| {
            assert_eq!(dir, expected_dir.as_path());
            assert_eq!(program, "git");
            assert_eq!(args, expected_args);
            Ok(ok_result(stdout))
        });
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

/// Build a context that uses a [`MockExecutor`] so we can control git responses.
fn make_update_context(config: crate::Config, executor: MockExecutor) -> Context {
    make_context(config, Platform::new(Os::Linux, false), Arc::new(executor))
}

#[test]
fn run_returns_skipped_when_detached_head() {
    let config = empty_config(PathBuf::from("/tmp"));
    // First call (symbolic-ref): fails → detached HEAD
    let mut mock = MockExecutor::new();
    mock.expect_run_in_with_env()
        .once()
        .returning(|_, _, _, _| Err(anyhow::anyhow!("simulated failure")));
    let ctx = make_update_context(config, mock);
    let repo_updated = UpdateSignal::new();
    let task = UpdateRepository::new(repo_updated.clone());

    let result = task.run(&ctx).unwrap();
    assert!(matches!(result, TaskResult::Skipped(ref s) if s.contains("detached HEAD")));
    assert!(!repo_updated.was_updated());
}

#[test]
fn run_skips_when_staged_changes_detected() {
    let config = empty_config(PathBuf::from("/tmp"));
    // First call (symbolic-ref): succeeds → on a branch
    // Second call (status --porcelain): returns non-empty stdout → local changes
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, _, _| Ok(ok_result("refs/heads/main")));
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, _, _| Ok(ok_result("M  dirty_file.txt")));
    let ctx = make_update_context(config, mock);
    let repo_updated = UpdateSignal::new();
    let task = UpdateRepository::new(repo_updated);

    let result = task.run(&ctx).unwrap();
    assert!(matches!(result, TaskResult::Skipped(ref s) if s.contains("local changes")));
}

#[derive(Debug)]
struct UntrackedAwareExecutor;

impl Executor for UntrackedAwareExecutor {
    fn run(&self, _: &str, _: &[&str]) -> Result<ExecResult> {
        anyhow::bail!("unexpected run() call")
    }

    fn run_in_with_env(
        &self,
        _: &Path,
        _: &str,
        args: &[&str],
        _: &[(&str, &str)],
    ) -> Result<ExecResult> {
        let stdout = if args.contains(&"--untracked-files=no") {
            String::new()
        } else {
            "?? new-file.txt\n".to_string()
        };

        Ok(ExecResult {
            stdout,
            stderr: String::new(),
            success: true,
            code: Some(0),
        })
    }

    fn run_unchecked(&self, _: &str, _: &[&str]) -> Result<ExecResult> {
        anyhow::bail!("unexpected run_unchecked() call")
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
fn run_returns_ok_and_does_not_mark_updated_when_already_up_to_date() {
    let config = empty_config(PathBuf::from("/tmp"));
    // 1. symbolic-ref → on a branch
    // 2. status → clean worktree
    // 3. fetch → ok
    // 4. rev-parse HEAD → SHA
    // 5. rev-parse @{u} → same SHA → already up to date
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    for stdout in ["refs/heads/main", "", "", "abc123", "abc123"] {
        let s = stdout.to_string();
        mock.expect_run_in_with_env()
            .once()
            .in_sequence(&mut seq)
            .returning(move |_, _, _, _| Ok(ok_result(&s)));
    }
    let ctx = make_update_context(config, mock);
    let repo_updated = UpdateSignal::new();
    let task = UpdateRepository::new(repo_updated.clone());

    let result = task.run(&ctx).unwrap();
    assert!(matches!(result, TaskResult::Ok));
    assert!(!repo_updated.was_updated());
}

#[test]
fn run_returns_ok_and_marks_updated_when_pull_fetches_new_commits() {
    let config = empty_config(PathBuf::from("/tmp"));
    // 1. symbolic-ref → on a branch
    // 2. status → clean worktree
    // 3. fetch → ok
    // 4. rev-parse HEAD → pre-merge SHA
    // 5. rev-parse @{u} → newer SHA
    // 6. rev-list --count @{u}..HEAD → 0 (not ahead)
    // 7. merge --ff-only → succeeds
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    for stdout in [
        "refs/heads/main",
        "",
        "",
        "abc1234",
        "def5678",
        "0",
        "Updating abc1234..def5678\nFast-forward",
    ] {
        let s = stdout.to_string();
        mock.expect_run_in_with_env()
            .once()
            .in_sequence(&mut seq)
            .returning(move |_, _, _, _| Ok(ok_result(&s)));
    }
    let ctx = make_update_context(config, mock);
    let repo_updated = UpdateSignal::new();
    let task = UpdateRepository::new(repo_updated.clone());

    let result = task.run(&ctx).unwrap();
    assert!(matches!(result, TaskResult::Ok));
    assert!(repo_updated.was_updated());
}

#[test]
fn run_returns_skipped_when_local_branch_diverged() {
    let config = empty_config(PathBuf::from("/tmp"));
    // 1. symbolic-ref → on a branch
    // 2. status → clean worktree
    // 3. fetch → ok
    // 4. rev-parse HEAD → SHA
    // 5. rev-parse @{u} → different SHA
    // 6. rev-list --count @{u}..HEAD → 2 (local commits ahead → diverged)
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    for stdout in ["refs/heads/main", "", "", "abc1234", "def5678", "2"] {
        let s = stdout.to_string();
        mock.expect_run_in_with_env()
            .once()
            .in_sequence(&mut seq)
            .returning(move |_, _, _, _| Ok(ok_result(&s)));
    }
    let ctx = make_update_context(config, mock);
    let repo_updated = UpdateSignal::new();
    let task = UpdateRepository::new(repo_updated.clone());

    let result = task.run(&ctx).unwrap();
    assert!(matches!(result, TaskResult::Skipped(ref s) if s.contains("diverged")));
    assert!(!repo_updated.was_updated());
}

#[test]
fn run_returns_skipped_when_rev_list_count_is_malformed() {
    let config = empty_config(PathBuf::from("/tmp"));
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    for stdout in [
        "refs/heads/main",
        "",
        "",
        "abc1234",
        "def5678",
        "not-a-count",
    ] {
        let output = stdout.to_string();
        mock.expect_run_in_with_env()
            .once()
            .in_sequence(&mut seq)
            .returning(move |_, _, _, _| Ok(ok_result(&output)));
    }
    let ctx = make_update_context(config, mock);
    let repo_updated = UpdateSignal::new();
    let task = UpdateRepository::new(repo_updated.clone());

    let result = task.run(&ctx).unwrap();

    assert!(
        matches!(result, TaskResult::Skipped(ref reason) if reason.contains("could not determine"))
    );
    assert!(!repo_updated.was_updated());
}

#[test]
fn run_returns_failed_when_fetch_fails() {
    let config = empty_config(PathBuf::from("/tmp"));
    // 1. symbolic-ref → on a branch
    // 2. status → clean worktree
    // 3. fetch → fails
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    for stdout in ["refs/heads/main", ""] {
        let s = stdout.to_string();
        mock.expect_run_in_with_env()
            .once()
            .in_sequence(&mut seq)
            .returning(move |_, _, _, _| Ok(ok_result(&s)));
    }
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, _, _| Err(anyhow::anyhow!("simulated fetch failure")));
    let ctx = make_update_context(config, mock);
    let repo_updated = UpdateSignal::new();
    let task = UpdateRepository::new(repo_updated);

    let result = task.run(&ctx).unwrap();
    assert!(matches!(result, TaskResult::Failed(ref s) if s.contains("git fetch failed")));
}

#[test]
fn run_retries_transient_fetch_failure() {
    let config = empty_config(PathBuf::from("/tmp"));
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    for stdout in ["refs/heads/main", ""] {
        let output = stdout.to_string();
        mock.expect_run_in_with_env()
            .once()
            .in_sequence(&mut seq)
            .returning(move |_, _, _, _| Ok(ok_result(&output)));
    }
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, _, _| {
            Err(anyhow::anyhow!(
                "mux_client_request_session: read from master failed: Connection reset by peer\n\
                 Failed to connect to new control master"
            ))
        });
    for stdout in ["", "abc123", "abc123"] {
        let output = stdout.to_string();
        mock.expect_run_in_with_env()
            .once()
            .in_sequence(&mut seq)
            .returning(move |_, _, _, _| Ok(ok_result(&output)));
    }
    let ctx = make_update_context(config, mock);
    let repo_updated = UpdateSignal::new();
    let task = UpdateRepository::new(repo_updated.clone());

    let result = task.run(&ctx).unwrap();

    assert!(matches!(result, TaskResult::Ok));
    assert!(!repo_updated.was_updated());
}

#[test]
fn run_stops_after_transient_fetch_retries_are_exhausted() {
    let config = empty_config(PathBuf::from("/tmp"));
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    for stdout in ["refs/heads/main", ""] {
        let output = stdout.to_string();
        mock.expect_run_in_with_env()
            .once()
            .in_sequence(&mut seq)
            .returning(move |_, _, _, _| Ok(ok_result(&output)));
    }
    mock.expect_run_in_with_env()
        .times(3)
        .in_sequence(&mut seq)
        .returning(|_, _, _, _| Err(anyhow::anyhow!("connection reset by peer")));
    let ctx = make_update_context(config, mock);
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

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    expect_git_success(
        &mut mock,
        &mut seq,
        main_root.clone(),
        &["symbolic-ref", "--quiet", "HEAD"],
        "refs/heads/main",
    );
    expect_git_success(
        &mut mock,
        &mut seq,
        main_root,
        &["status", "--porcelain", "--untracked-files=no"],
        "",
    );
    expect_git_success(
        &mut mock,
        &mut seq,
        overlay_root.clone(),
        &["symbolic-ref", "--quiet", "HEAD"],
        "refs/heads/main",
    );
    expect_git_success(
        &mut mock,
        &mut seq,
        overlay_root,
        &["status", "--porcelain", "--untracked-files=no"],
        "M  private.toml",
    );

    let ctx = make_update_context(config, mock);
    let repo_updated = UpdateSignal::new();
    let task = UpdateRepository::new(repo_updated.clone());

    let result = task.run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Skipped(ref s) if s.contains("local changes") && s.contains("overlay"))
    );
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

    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    expect_git_success(
        &mut mock,
        &mut seq,
        main_root.clone(),
        &["symbolic-ref", "--quiet", "HEAD"],
        "refs/heads/main",
    );
    expect_git_success(
        &mut mock,
        &mut seq,
        main_root.clone(),
        &["status", "--porcelain", "--untracked-files=no"],
        "",
    );
    expect_git_success(
        &mut mock,
        &mut seq,
        overlay_root.clone(),
        &["symbolic-ref", "--quiet", "HEAD"],
        "refs/heads/main",
    );
    expect_git_success(
        &mut mock,
        &mut seq,
        overlay_root.clone(),
        &["status", "--porcelain", "--untracked-files=no"],
        "",
    );
    expect_git_success(
        &mut mock,
        &mut seq,
        main_root.clone(),
        &["fetch", "--quiet"],
        "",
    );
    expect_git_success(
        &mut mock,
        &mut seq,
        overlay_root.clone(),
        &["fetch", "--quiet"],
        "",
    );
    expect_git_success(
        &mut mock,
        &mut seq,
        main_root.clone(),
        &["rev-parse", "HEAD"],
        "abc123",
    );
    expect_git_success(
        &mut mock,
        &mut seq,
        main_root,
        &["rev-parse", "@{u}"],
        "abc123",
    );
    expect_git_success(
        &mut mock,
        &mut seq,
        overlay_root.clone(),
        &["rev-parse", "HEAD"],
        "def456",
    );
    expect_git_success(
        &mut mock,
        &mut seq,
        overlay_root.clone(),
        &["rev-parse", "@{u}"],
        "fed654",
    );
    expect_git_success(
        &mut mock,
        &mut seq,
        overlay_root.clone(),
        &["rev-list", "--count", "@{u}..HEAD"],
        "0",
    );
    expect_git_success(
        &mut mock,
        &mut seq,
        overlay_root,
        &["merge", "--ff-only", "@{u}"],
        "Updating def456..fed654\nFast-forward",
    );

    let ctx = make_update_context(config, mock);
    let repo_updated = UpdateSignal::new();
    let task = UpdateRepository::new(repo_updated.clone());

    let result = task.run(&ctx).unwrap();
    assert!(matches!(result, TaskResult::Ok));
    assert!(repo_updated.was_updated());
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
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    for stdout in [
        "refs/heads/main",
        "",
        "abc123",
        "origin",
        "refs/heads/main",
        "abc123\trefs/heads/main",
    ] {
        let s = stdout.to_string();
        mock.expect_run_in_with_env()
            .once()
            .in_sequence(&mut seq)
            .returning(move |_, _, _, _| Ok(ok_result(&s)));
    }
    let mut ctx = make_update_context(config, mock);
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
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    for stdout in [
        "refs/heads/main",
        "",
        "abc123",
        "origin",
        "refs/heads/main",
        "def456\trefs/heads/main",
    ] {
        let s = stdout.to_string();
        mock.expect_run_in_with_env()
            .once()
            .in_sequence(&mut seq)
            .returning(move |_, _, _, _| Ok(ok_result(&s)));
    }
    let mut ctx = make_update_context(config, mock);
    ctx = ctx.with_dry_run(true);
    let task = UpdateRepository::new(UpdateSignal::new());

    let result = task.run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::DryRun),
        "expected DryRun (behind upstream), got {result:?}"
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
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    for stdout in ["refs/heads/main", "", "abc123"] {
        let s = stdout.to_string();
        mock.expect_run_in_with_env()
            .once()
            .in_sequence(&mut seq)
            .returning(move |_, _, _, _| Ok(ok_result(&s)));
    }
    // branch.main.remote lookup fails
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, _, _| Err(anyhow::anyhow!("no remote config")));
    // rev-parse @{u}: matches HEAD
    mock.expect_run_in_with_env()
        .once()
        .in_sequence(&mut seq)
        .returning(|_, _, _, _| Ok(ok_result("abc123")));
    let mut ctx = make_update_context(config, mock);
    ctx = ctx.with_dry_run(true);
    let task = UpdateRepository::new(UpdateSignal::new());

    let result = task.run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok when cached upstream matches HEAD, got {result:?}"
    );
}
