use anyhow::Result;

use super::helpers::ConfigBatchProcessor;
use super::{Context, Task, TaskResult};
use crate::exec;

/// Install VS Code extensions.
pub struct InstallVsCodeExtensions;

impl Task for InstallVsCodeExtensions {
    fn name(&self) -> &'static str {
        "Install VS Code extensions"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config.vscode_extensions.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        // Find the VS Code CLI binary
        let code_cmd = find_code_command();
        let Some(cmd) = code_cmd else {
            ctx.log
                .debug("neither code-insiders nor code found in PATH");
            return Ok(TaskResult::Skipped("VS Code CLI not found".to_string()));
        };

        ctx.log.debug(&format!("using VS Code CLI: {cmd}"));

        let mut processor = ConfigBatchProcessor::new();

        ctx.log.debug("listing installed extensions");
        let installed = run_code_cmd(&cmd, &["--list-extensions"])
            .map(|r| r.stdout.to_lowercase())
            .unwrap_or_default();
        ctx.log.debug(&format!(
            "found {} installed extensions",
            installed.lines().count()
        ));

        for ext in &ctx.config.vscode_extensions {
            if installed.contains(&ext.id.to_lowercase()) {
                ctx.log
                    .debug(&format!("ok: {} (already installed)", ext.id));
                processor.stats.already_ok += 1;
                continue;
            }

            if ctx.dry_run {
                ctx.log
                    .dry_run(&format!("would install extension: {}", ext.id));
                processor.stats.changed += 1;
                continue;
            }

            ctx.log.debug(&format!("installing extension: {}", ext.id));
            let result = run_code_cmd(&cmd, &["--install-extension", &ext.id, "--force"])?;
            if result.success {
                ctx.log.debug(&format!("installed extension: {}", ext.id));
                processor.stats.changed += 1;
            } else {
                ctx.log
                    .warn(&format!("failed to install extension: {}", ext.id));
            }
        }

        Ok(processor.finish(ctx))
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

/// Run a VS Code CLI command. On Windows, `.cmd` wrappers need `cmd.exe /C`.
fn run_code_cmd(cmd: &str, args: &[&str]) -> anyhow::Result<exec::ExecResult> {
    #[cfg(target_os = "windows")]
    {
        let mut full_args = vec!["/C", cmd];
        full_args.extend(args);
        exec::run_unchecked("cmd", &full_args)
    }

    #[cfg(not(target_os = "windows"))]
    {
        exec::run_unchecked(cmd, args)
    }
}
