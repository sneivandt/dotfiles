use anyhow::Result;

use super::{Context, Task, TaskResult};
use crate::exec;

/// Configure the default shell to zsh.
pub struct ConfigureShell;

impl Task for ConfigureShell {
    fn name(&self) -> &'static str {
        "Configure default shell"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_linux() && exec::which("zsh")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        // Check current shell
        let current_shell = std::env::var("SHELL").unwrap_or_default();
        ctx.log.debug(&format!("current shell: {current_shell:?}"));
        if current_shell.ends_with("/zsh") {
            ctx.log.info("zsh is already default");
            return Ok(TaskResult::Ok);
        }

        let zsh_path = exec::run("which", &["zsh"])?;
        let zsh_path = zsh_path.stdout.trim();
        ctx.log.debug(&format!("zsh binary: {zsh_path}"));

        if ctx.dry_run {
            ctx.log.dry_run(&format!("chsh -s {zsh_path}"));
            return Ok(TaskResult::DryRun);
        }

        // Try to change shell, but don't fail if chsh requires authentication (e.g., in CI)
        match exec::run("chsh", &["-s", zsh_path]) {
            Ok(_) => {
                ctx.log.info("default shell changed to zsh");
                Ok(TaskResult::Ok)
            }
            Err(e) => {
                let err_msg = e.to_string();
                // Check if failure is due to authentication (common in CI environments)
                if err_msg.contains("PAM:") || err_msg.contains("Authentication") {
                    ctx.log
                        .warn("cannot change shell without authentication (skipping in CI)");
                    Ok(TaskResult::Skipped(
                        "chsh requires authentication".to_string(),
                    ))
                } else {
                    Err(e)
                }
            }
        }
    }
}
