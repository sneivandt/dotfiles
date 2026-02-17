use anyhow::Result;

use super::{Context, Task, TaskResult};
use crate::exec;

/// Configure the default shell to zsh.
pub struct ConfigureShell;

impl Task for ConfigureShell {
    fn name(&self) -> &str {
        "Configure default shell"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_linux() && exec::which("zsh")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        // Check current shell
        let current_shell = std::env::var("SHELL").unwrap_or_default();
        if current_shell.ends_with("/zsh") {
            ctx.log.info("zsh is already default");
            return Ok(TaskResult::Ok);
        }

        let zsh_path = exec::run("which", &["zsh"])?;
        let zsh_path = zsh_path.stdout.trim();

        if ctx.dry_run {
            ctx.log.dry_run(&format!("chsh -s {zsh_path}"));
            return Ok(TaskResult::DryRun);
        }

        exec::run("chsh", &["-s", zsh_path])?;
        ctx.log.info("default shell changed to zsh");
        Ok(TaskResult::Ok)
    }
}
