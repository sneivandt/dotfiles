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
            ctx.log.dry_run("git pull");
            return Ok(TaskResult::DryRun);
        }

        let result = exec::run_in(ctx.root(), "git", &["pull", "--ff-only"]);
        match result {
            Ok(r) => {
                let msg = r.stdout.trim().to_string();
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
