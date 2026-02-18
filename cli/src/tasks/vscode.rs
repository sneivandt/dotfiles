use anyhow::Result;

use super::{Context, Task, TaskResult, TaskStats};
use crate::resources::vscode_extension::{
    VsCodeExtensionResource, find_code_command, get_installed_extensions,
};
use crate::resources::{Resource, ResourceChange, ResourceState};

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
        let Some(cmd) = find_code_command() else {
            ctx.log
                .debug("neither code-insiders nor code found in PATH");
            return Ok(TaskResult::Skipped("VS Code CLI not found".to_string()));
        };

        ctx.log.debug(&format!("using VS Code CLI: {cmd}"));

        // Single invocation to list all installed extensions
        ctx.log.debug(&format!(
            "batch-checking {} extensions with a single query",
            ctx.config.vscode_extensions.len()
        ));
        let installed = get_installed_extensions(&cmd)?;

        let mut stats = TaskStats::new();

        for ext in &ctx.config.vscode_extensions {
            let resource = VsCodeExtensionResource::new(ext.id.clone(), cmd.clone());
            let resource_state = resource.state_from_installed(&installed);

            match resource_state {
                ResourceState::Correct => {
                    ctx.log.debug(&format!(
                        "ok: {} (already installed)",
                        resource.description()
                    ));
                    stats.already_ok += 1;
                }
                ResourceState::Missing => {
                    if ctx.dry_run {
                        ctx.log.dry_run(&format!(
                            "would install extension: {}",
                            resource.description()
                        ));
                        stats.changed += 1;
                        continue;
                    }

                    ctx.log
                        .debug(&format!("installing extension: {}", resource.description()));
                    match resource.apply() {
                        Ok(ResourceChange::Applied) => {
                            ctx.log
                                .debug(&format!("installed extension: {}", resource.description()));
                            stats.changed += 1;
                        }
                        Ok(ResourceChange::Skipped { reason }) => {
                            ctx.log.warn(&format!(
                                "failed to install extension {}: {reason}",
                                resource.description()
                            ));
                        }
                        Ok(ResourceChange::AlreadyCorrect) => {
                            stats.already_ok += 1;
                        }
                        Err(e) => {
                            ctx.log.warn(&format!(
                                "failed to install extension {}: {e}",
                                resource.description()
                            ));
                        }
                    }
                }
                ResourceState::Incorrect { current } => {
                    ctx.log.debug(&format!(
                        "extension {} unexpected state: {current}",
                        resource.description()
                    ));
                    stats.skipped += 1;
                }
                ResourceState::Invalid { reason } => {
                    ctx.log
                        .debug(&format!("skipping {}: {reason}", resource.description()));
                    stats.skipped += 1;
                }
            }
        }

        Ok(stats.finish(ctx))
    }
}
