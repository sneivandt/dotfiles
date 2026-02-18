use anyhow::Result;

use super::{Context, Task, TaskResult};
use crate::exec;

/// Pull latest changes from the remote repository.
pub struct UpdateRepository;

impl Task for UpdateRepository {
    fn name(&self) -> &'static str {
        "Update repository"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.root().join(".git").exists()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if ctx.dry_run {
            // Compare local HEAD with upstream tracking branch
            if let (Some(head), Some(upstream)) = (
                exec::run_in(ctx.root(), "git", &["rev-parse", "HEAD"]).ok(),
                exec::run_in(ctx.root(), "git", &["rev-parse", "@{u}"]).ok(),
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
        let result = exec::run_in(ctx.root(), "git", &["pull", "--ff-only"]);
        match result {
            Ok(r) => {
                let msg = r.stdout.trim().to_string();
                ctx.log.debug(&format!("git pull output: {msg}"));
                if msg.contains("Already up to date") {
                    ctx.log.info("already up to date");
                } else {
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
