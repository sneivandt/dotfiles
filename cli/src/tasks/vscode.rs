use anyhow::Result;

use super::{Context, Task, TaskResult};
use crate::exec;

/// Install VS Code extensions.
pub struct InstallVsCodeExtensions;

impl Task for InstallVsCodeExtensions {
    fn name(&self) -> &str {
        "Install VS Code extensions"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config.vscode_extensions.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        // Find the VS Code CLI binary
        let code_cmd = find_code_command();
        let Some(cmd) = code_cmd else {
            return Ok(TaskResult::Skipped("VS Code CLI not found".to_string()));
        };

        let mut count = 0u32;
        let mut skipped = 0u32;

        if ctx.dry_run {
            let installed = exec::run_unchecked(&cmd, &["--list-extensions"])
                .map(|r| r.stdout.to_lowercase())
                .unwrap_or_default();

            for ext in &ctx.config.vscode_extensions {
                if installed.contains(&ext.id.to_lowercase()) {
                    ctx.log
                        .debug(&format!("ok: {} (already installed)", ext.id));
                    skipped += 1;
                } else {
                    ctx.log
                        .dry_run(&format!("would install extension: {}", ext.id));
                    count += 1;
                }
            }
            ctx.log
                .info(&format!("{count} would change, {skipped} already ok"));
            return Ok(TaskResult::DryRun);
        }

        for ext in &ctx.config.vscode_extensions {
            let result = exec::run_unchecked(&cmd, &["--install-extension", &ext.id, "--force"])?;
            if result.success {
                count += 1;
            } else {
                ctx.log
                    .debug(&format!("extension may already be installed: {}", ext.id));
            }
        }

        ctx.log.info(&format!("{count} extensions installed"));
        Ok(TaskResult::Ok)
    }
}

fn find_code_command() -> Option<String> {
    for cmd in &["code-insiders", "code"] {
        if exec::which(cmd) {
            return Some((*cmd).to_string());
        }
    }
    None
}
