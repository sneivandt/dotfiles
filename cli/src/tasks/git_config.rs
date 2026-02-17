use anyhow::Result;

use super::{Context, Task, TaskResult};
use crate::exec;

/// Configure git settings (Windows-specific git config).
pub struct ConfigureGit;

impl Task for ConfigureGit {
    fn name(&self) -> &str {
        "Configure git"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_windows() && exec::which("git")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if ctx.dry_run {
            ctx.log.dry_run("configure git settings");
            return Ok(TaskResult::DryRun);
        }

        // Set core.autocrlf to false on Windows
        exec::run("git", &["config", "--global", "core.autocrlf", "false"])?;

        // Set credential helper
        exec::run(
            "git",
            &["config", "--global", "credential.helper", "manager"],
        )?;

        ctx.log.info("git configured");
        Ok(TaskResult::Ok)
    }
}
