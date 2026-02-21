use anyhow::Result;
use std::any::TypeId;

use super::{Context, Task, TaskResult};

/// Pull latest changes from the remote repository.
#[derive(Debug)]
pub struct UpdateRepository;

impl Task for UpdateRepository {
    fn name(&self) -> &'static str {
        "Update repository"
    }

    fn dependencies(&self) -> &[TypeId] {
        const DEPS: &[TypeId] = &[TypeId::of::<super::sparse_checkout::ConfigureSparseCheckout>()];
        DEPS
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.root().join(".git").exists()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        // Pass HOME so git finds the correct global config when running elevated
        // on Windows (elevated token can have a different home path).
        let home_str = ctx.home.to_string_lossy().to_string();
        let git_env: &[(&str, &str)] = &[("HOME", &home_str), ("GIT_CONFIG_NOSYSTEM", "1")];

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
                    ctx.repo_updated
                        .store(true, std::sync::atomic::Ordering::Release);
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
    use crate::exec::Executor;
    use crate::platform::{Os, Platform};
    use crate::tasks::test_helpers::{NoOpExecutor, empty_config, make_context};
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn should_run_false_when_git_dir_missing() {
        let config = empty_config(PathBuf::from("/nonexistent/repo"));
        let platform = Arc::new(Platform::new(Os::Linux, false));
        let executor: Arc<dyn Executor> = Arc::new(NoOpExecutor);
        let ctx = make_context(config, platform, executor);
        assert!(!UpdateRepository.should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_git_dir_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let config = empty_config(dir.path().to_path_buf());
        let platform = Arc::new(Platform::new(Os::Linux, false));
        let executor: Arc<dyn Executor> = Arc::new(NoOpExecutor);
        let ctx = make_context(config, platform, executor);
        assert!(UpdateRepository.should_run(&ctx));
    }
}
