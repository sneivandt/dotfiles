//! Task: update the dotfiles repository.
use anyhow::Result;

use super::{Context, Task, TaskResult, task_deps, update_signal::UpdateSignal};

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
        if ctx
            .executor
            .run_in_with_env(
                &ctx.root(),
                "git",
                &["symbolic-ref", "--quiet", "HEAD"],
                git_env,
            )
            .is_err()
        {
            ctx.log.info("detached HEAD, skipping pull");
            return Ok(TaskResult::Skipped("detached HEAD".to_string()));
        }

        // Refuse to pull if there are staged changes that could be lost
        if let Ok(diff) = ctx.executor.run_in_with_env(
            &ctx.root(),
            "git",
            &["diff", "--cached", "--name-only"],
            git_env,
        ) && !diff.stdout.trim().is_empty()
        {
            ctx.log.warn("staged changes detected, skipping update");
            return Ok(TaskResult::Skipped("staged changes present".to_string()));
        }

        if ctx.dry_run {
            // Compare local HEAD with upstream tracking branch
            if let (Some(head), Some(upstream)) = (
                ctx.executor
                    .run_in_with_env(&ctx.root(), "git", &["rev-parse", "HEAD"], git_env)
                    .ok(),
                ctx.executor
                    .run_in_with_env(&ctx.root(), "git", &["rev-parse", "@{u}"], git_env)
                    .ok(),
            ) && head.stdout.trim() == upstream.stdout.trim()
            {
                ctx.log.info("already up to date");
                return Ok(TaskResult::Ok);
            }
            ctx.log.dry_run("git pull");
            return Ok(TaskResult::DryRun);
        }

        ctx.log
            .debug(&format!("pulling from {}", ctx.root().display()));
        let result =
            ctx.executor
                .run_in_with_env(&ctx.root(), "git", &["pull", "--ff-only"], git_env);
        match result {
            Ok(r) => {
                let msg = r.stdout.trim().to_string();
                ctx.log.debug(&format!("git pull output: {msg}"));
                if msg.contains("Already up to date") {
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::platform::{Os, Platform};
    use crate::resources::test_helpers::MockExecutor;
    use crate::tasks::test_helpers::{empty_config, make_context, make_linux_context};
    use crate::tasks::update_signal::UpdateSignal;
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

    /// Build a context that uses a [`MockExecutor`] so we can control git responses.
    fn make_update_context(config: crate::config::Config, executor: MockExecutor) -> Context {
        make_context(
            config,
            Arc::new(Platform::new(Os::Linux, false)),
            Arc::new(executor),
        )
    }

    #[test]
    fn run_returns_skipped_when_detached_head() {
        let config = empty_config(PathBuf::from("/tmp"));
        // First call (symbolic-ref): fails → detached HEAD
        let executor = MockExecutor::fail();
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
        // Second call (diff --cached): returns non-empty stdout → staged changes
        let executor = MockExecutor::with_responses(vec![
            (true, "refs/heads/main".to_string()),
            (true, "dirty_file.txt".to_string()),
        ]);
        let ctx = make_update_context(config, executor);
        let repo_updated = UpdateSignal::new();
        let task = UpdateRepository::new(repo_updated);

        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Skipped(ref s) if s.contains("staged changes")));
    }

    #[test]
    fn run_returns_ok_and_does_not_mark_updated_when_already_up_to_date() {
        let config = empty_config(PathBuf::from("/tmp"));
        // First call (symbolic-ref): succeeds → on a branch
        // Second call (diff): empty stdout → no staged changes
        // Third call (pull): "Already up to date."
        let executor = MockExecutor::with_responses(vec![
            (true, "refs/heads/main".to_string()),
            (true, String::new()),
            (true, "Already up to date.".to_string()),
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
        // Second call (diff): empty stdout → no staged changes
        // Third call (pull): update output → repo was updated
        let executor = MockExecutor::with_responses(vec![
            (true, "refs/heads/main".to_string()),
            (true, String::new()),
            (true, "Updating abc1234..def5678\nFast-forward".to_string()),
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
        // Second call (diff): empty stdout → no staged changes
        // Third call (pull): fails
        let executor = MockExecutor::with_responses(vec![
            (true, "refs/heads/main".to_string()),
            (true, String::new()),
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
        // diff --cached: empty → no staged changes
        // rev-parse HEAD: abc123
        // rev-parse @{u}: abc123 (same SHA → already up to date)
        let executor = MockExecutor::with_responses(vec![
            (true, "refs/heads/main".to_string()),
            (true, String::new()),
            (true, "abc123".to_string()),
            (true, "abc123".to_string()),
        ]);
        let mut ctx = make_update_context(config, executor);
        ctx.dry_run = true;
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
        // diff --cached: empty
        // rev-parse HEAD: abc123
        // rev-parse @{u}: def456 (different SHA → would pull)
        let executor = MockExecutor::with_responses(vec![
            (true, "refs/heads/main".to_string()),
            (true, String::new()),
            (true, "abc123".to_string()),
            (true, "def456".to_string()),
        ]);
        let mut ctx = make_update_context(config, executor);
        ctx.dry_run = true;
        let task = UpdateRepository::new(UpdateSignal::new());

        let result = task.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::DryRun),
            "expected DryRun (behind upstream), got {result:?}"
        );
    }
}
