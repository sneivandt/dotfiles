use anyhow::Result;
use std::any::TypeId;

use super::{Context, Task, TaskResult};
use crate::exec;

/// Configure the default shell to zsh.
pub struct ConfigureShell;

impl Task for ConfigureShell {
    fn name(&self) -> &'static str {
        "Configure default shell"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        // Skip in CI environments where chsh requires authentication
        let is_ci = std::env::var("CI").is_ok();
        ctx.platform.is_linux() && exec::which("zsh") && !is_ci
    }

    fn dependencies(&self) -> Vec<TypeId> {
        vec![TypeId::of::<super::packages::InstallPackages>()]
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

        exec::run("chsh", &["-s", zsh_path])?;
        ctx.log.info("default shell changed to zsh");
        Ok(TaskResult::Ok)
    }
}
