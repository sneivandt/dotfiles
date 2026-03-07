//! Task: update the dotfiles repository.
use anyhow::Result;

use super::{Context, Task, TaskResult, UpdateSignal, task_deps};

/// Pull latest changes from the remote repository.
#[derive(Debug)]
pub struct UpdateRepository {
    /// Set to `true` when the repository is actually updated by this task.
    ///
    /// Shared with [`super::reload_config::ReloadConfig`] so that task can
    /// skip the reload when the repository was already up to date.
    pub(super) repo_updated: UpdateSignal,
}

impl UpdateRepository {
    /// Create a new task, sharing `repo_updated` with `ReloadConfig`.
    #[must_use]
    pub const fn new(repo_updated: UpdateSignal) -> Self {
        Self { repo_updated }
    }
}

impl Task for UpdateRepository {
    fn name(&self) -> &'static str {
        "Update repository"
    }

    task_deps![super::sparse_checkout::ConfigureSparseCheckout];

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.root().join(".git").exists()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        // Pass HOME so git finds the correct global config when running elevated
        // on Windows (elevated token can have a different home path).
        let home_str = ctx.home.to_string_lossy().to_string();
        let git_env: &[(&str, &str)] = &[("HOME", &home_str), ("GIT_CONFIG_NOSYSTEM", "1")];

        // Skip when not on a branch (e.g. detached HEAD in CI checkouts).
        let head_ref = if let Ok(result) = ctx.executor.run_in_with_env(
            &ctx.root(),
            "git",
            &["symbolic-ref", "--quiet", "HEAD"],
            git_env,
        ) {
            result.stdout.trim().to_string()
        } else {
            ctx.log.info("detached HEAD, skipping pull");
            return Ok(TaskResult::Skipped("detached HEAD".to_string()));
        };

        // Refuse to pull when the worktree is dirty (staged, unstaged, or
        // untracked files). That keeps update failures predictable and avoids
        // mixing local edits with a repository fast-forward.
        if worktree_has_local_changes(ctx, git_env)? {
            ctx.log.warn("local changes detected, skipping update");
            return Ok(TaskResult::Skipped("local changes present".to_string()));
        }

        if ctx.dry_run {
            match dry_run_update_status(ctx, git_env, &head_ref)? {
                DryRunUpdateStatus::AlreadyCurrent => {
                    ctx.log.info("already up to date");
                    return Ok(TaskResult::Ok);
                }
                DryRunUpdateStatus::WouldUpdate | DryRunUpdateStatus::Unknown => {
                    ctx.log.dry_run("git pull");
                    return Ok(TaskResult::DryRun);
                }
            }
        }

        ctx.log
            .debug(&format!("pulling from {}", ctx.root().display()));

        let pre_sha = ctx
            .executor
            .run_in_with_env(&ctx.root(), "git", &["rev-parse", "HEAD"], git_env)?
            .stdout
            .trim()
            .to_string();

        let result =
            ctx.executor
                .run_in_with_env(&ctx.root(), "git", &["pull", "--ff-only"], git_env);
        match result {
            Ok(r) => {
                ctx.log
                    .debug(&format!("git pull output: {}", r.stdout.trim()));

                let post_sha = ctx
                    .executor
                    .run_in_with_env(&ctx.root(), "git", &["rev-parse", "HEAD"], git_env)?
                    .stdout
                    .trim()
                    .to_string();

                if pre_sha == post_sha {
                    ctx.log.info("already up to date");
                } else {
                    self.repo_updated.mark_updated();
                    ctx.log.info("repository updated");
                }
                Ok(TaskResult::Ok)
            }
            Err(e) => {
                ctx.log.warn(&format!("git pull failed: {e:#}"));
                Ok(TaskResult::Skipped("git pull failed".to_string()))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DryRunUpdateStatus {
    AlreadyCurrent,
    WouldUpdate,
    Unknown,
}

fn dry_run_update_status(
    ctx: &Context,
    git_env: &[(&str, &str)],
    head_ref: &str,
) -> Result<DryRunUpdateStatus> {
    let head = ctx
        .executor
        .run_in_with_env(&ctx.root(), "git", &["rev-parse", "HEAD"], git_env)?;
    let head_sha = head.stdout.trim().to_string();

    if let Some(remote_sha) = upstream_remote_sha(ctx, git_env, head_ref) {
        return Ok(if head_sha == remote_sha {
            DryRunUpdateStatus::AlreadyCurrent
        } else {
            DryRunUpdateStatus::WouldUpdate
        });
    }

    if let Ok(upstream) =
        ctx.executor
            .run_in_with_env(&ctx.root(), "git", &["rev-parse", "@{u}"], git_env)
    {
        return Ok(if head_sha == upstream.stdout.trim() {
            DryRunUpdateStatus::Unknown
        } else {
            DryRunUpdateStatus::WouldUpdate
        });
    }

    Ok(DryRunUpdateStatus::Unknown)
}

fn upstream_remote_sha(ctx: &Context, git_env: &[(&str, &str)], head_ref: &str) -> Option<String> {
    let branch = head_ref.strip_prefix("refs/heads/").unwrap_or(head_ref);
    let remote_key = format!("branch.{branch}.remote");
    let merge_key = format!("branch.{branch}.merge");

    let remote = ctx
        .executor
        .run_in_with_env(
            &ctx.root(),
            "git",
            &["config", "--get", &remote_key],
            git_env,
        )
        .ok()?;
    let merge_ref = ctx
        .executor
        .run_in_with_env(
            &ctx.root(),
            "git",
            &["config", "--get", &merge_key],
            git_env,
        )
        .ok()?;

    let remote_name = remote.stdout.trim();
    let merge_name = merge_ref.stdout.trim();
    if remote_name.is_empty() || merge_name.is_empty() {
        return None;
    }

    let ls_remote = ctx
        .executor
        .run_in_with_env(
            &ctx.root(),
            "git",
            &["ls-remote", "--exit-code", remote_name, merge_name],
            git_env,
        )
        .ok()?;

    ls_remote
        .stdout
        .split_whitespace()
        .next()
        .map(ToString::to_string)
}

fn worktree_has_local_changes(ctx: &Context, git_env: &[(&str, &str)]) -> Result<bool> {
    let status = ctx.executor.run_in_with_env(
        &ctx.root(),
        "git",
        &["status", "--porcelain", "--untracked-files=normal"],
        git_env,
    )?;

    Ok(!status.stdout.trim().is_empty())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::exec::test_helpers::TestExecutor;
    use crate::platform::{Os, Platform};
    use crate::tasks::UpdateSignal;
    use crate::tasks::test_helpers::{empty_config, make_context, make_linux_context};
    use std::path::PathBuf;
    use std::sync::Arc;

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

    // -----------------------------------------------------------------------
    // run()
    // -----------------------------------------------------------------------

    /// Build a context that uses a [`TestExecutor`] so we can control git responses.
    fn make_update_context(config: crate::config::Config, executor: TestExecutor) -> Context {
        make_context(config, Platform::new(Os::Linux, false), Arc::new(executor))
    }

    #[test]
    fn run_returns_skipped_when_detached_head() {
        let config = empty_config(PathBuf::from("/tmp"));
        // First call (symbolic-ref): fails → detached HEAD
        let executor = TestExecutor::fail();
        let ctx = make_update_context(config, executor);
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
        let executor = TestExecutor::with_responses(vec![
            (true, "refs/heads/main".to_string()),
            (true, "M  dirty_file.txt".to_string()),
        ]);
        let ctx = make_update_context(config, executor);
        let repo_updated = UpdateSignal::new();
        let task = UpdateRepository::new(repo_updated);

        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Skipped(ref s) if s.contains("local changes")));
    }

    #[test]
    fn run_skips_when_untracked_files_detected() {
        let config = empty_config(PathBuf::from("/tmp"));
        let executor = TestExecutor::with_responses(vec![
            (true, "refs/heads/main".to_string()),
            (true, "?? new-file.txt".to_string()),
        ]);
        let ctx = make_update_context(config, executor);
        let task = UpdateRepository::new(UpdateSignal::new());

        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Skipped(ref s) if s.contains("local changes")));
    }

    #[test]
    fn run_returns_ok_and_does_not_mark_updated_when_already_up_to_date() {
        let config = empty_config(PathBuf::from("/tmp"));
        // First call (symbolic-ref): succeeds → on a branch
        // Second call (status): empty stdout → clean worktree
        // Third call (rev-parse HEAD): pre-pull SHA
        // Fourth call (pull): succeeds
        // Fifth call (rev-parse HEAD): same SHA → no update
        let executor = TestExecutor::with_responses(vec![
            (true, "refs/heads/main".to_string()),
            (true, String::new()),
            (true, "abc123".to_string()),
            (true, String::new()),
            (true, "abc123".to_string()),
        ]);
        let ctx = make_update_context(config, executor);
        let repo_updated = UpdateSignal::new();
        let task = UpdateRepository::new(repo_updated.clone());

        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));
        assert!(!repo_updated.was_updated());
    }

    #[test]
    fn run_returns_ok_and_marks_updated_when_pull_fetches_new_commits() {
        let config = empty_config(PathBuf::from("/tmp"));
        // First call (symbolic-ref): succeeds → on a branch
        // Second call (status): empty stdout → clean worktree
        // Third call (rev-parse HEAD): pre-pull SHA
        // Fourth call (pull): succeeds with update output
        // Fifth call (rev-parse HEAD): different SHA → repo updated
        let executor = TestExecutor::with_responses(vec![
            (true, "refs/heads/main".to_string()),
            (true, String::new()),
            (true, "abc1234".to_string()),
            (true, "Updating abc1234..def5678\nFast-forward".to_string()),
            (true, "def5678".to_string()),
        ]);
        let ctx = make_update_context(config, executor);
        let repo_updated = UpdateSignal::new();
        let task = UpdateRepository::new(repo_updated.clone());

        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));
        assert!(repo_updated.was_updated());
    }

    #[test]
    fn run_returns_skipped_when_pull_fails() {
        let config = empty_config(PathBuf::from("/tmp"));
        // First call (symbolic-ref): succeeds → on a branch
        // Second call (status): empty stdout → clean worktree
        // Third call (rev-parse HEAD): pre-pull SHA
        // Fourth call (pull): fails
        let executor = TestExecutor::with_responses(vec![
            (true, "refs/heads/main".to_string()),
            (true, String::new()),
            (true, "abc123".to_string()),
            (false, String::new()),
        ]);
        let ctx = make_update_context(config, executor);
        let repo_updated = UpdateSignal::new();
        let task = UpdateRepository::new(repo_updated);

        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Skipped(ref s) if s.contains("git pull failed")));
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
        let executor = TestExecutor::with_responses(vec![
            (true, "refs/heads/main".to_string()),
            (true, String::new()),
            (true, "abc123".to_string()),
            (true, "origin".to_string()),
            (true, "refs/heads/main".to_string()),
            (true, "abc123\trefs/heads/main".to_string()),
        ]);
        let mut ctx = make_update_context(config, executor);
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
        let executor = TestExecutor::with_responses(vec![
            (true, "refs/heads/main".to_string()),
            (true, String::new()),
            (true, "abc123".to_string()),
            (true, "origin".to_string()),
            (true, "refs/heads/main".to_string()),
            (true, "def456\trefs/heads/main".to_string()),
        ]);
        let mut ctx = make_update_context(config, executor);
        ctx = ctx.with_dry_run(true);
        let task = UpdateRepository::new(UpdateSignal::new());

        let result = task.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::DryRun),
            "expected DryRun (behind upstream), got {result:?}"
        );
    }

    #[test]
    fn run_dry_run_returns_dry_run_when_remote_status_is_unknown() {
        let config = empty_config(PathBuf::from("/tmp"));
        // symbolic-ref: success
        // status --porcelain: empty
        // rev-parse HEAD: abc123
        // branch.main.remote lookup fails
        // rev-parse @{u}: abc123 (cached tracking ref matches, but remote was not verified)
        let executor = TestExecutor::with_responses(vec![
            (true, "refs/heads/main".to_string()),
            (true, String::new()),
            (true, "abc123".to_string()),
            (false, String::new()),
            (true, "abc123".to_string()),
        ]);
        let mut ctx = make_update_context(config, executor);
        ctx = ctx.with_dry_run(true);
        let task = UpdateRepository::new(UpdateSignal::new());

        let result = task.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::DryRun),
            "expected DryRun when remote status is unknown, got {result:?}"
        );
    }
}
