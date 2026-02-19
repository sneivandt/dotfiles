use anyhow::Result;

use super::{Context, ProcessOpts, Task, TaskResult, process_resource_states};
use crate::resources::vscode_extension::{
    VsCodeExtensionResource, find_code_command, get_installed_extensions,
};

/// Install VS Code extensions.
#[derive(Debug)]
pub struct InstallVsCodeExtensions;

impl Task for InstallVsCodeExtensions {
    fn name(&self) -> &'static str {
        "Install VS Code extensions"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config.vscode_extensions.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let Some(cmd) = find_code_command() else {
            ctx.log
                .debug("neither code-insiders nor code found in PATH");
            return Ok(TaskResult::Skipped("VS Code CLI not found".to_string()));
        };

        ctx.log.debug(&format!("using VS Code CLI: {cmd}"));
        ctx.log.debug(&format!(
            "batch-checking {} extensions with a single query",
            ctx.config.vscode_extensions.len()
        ));
        let installed = get_installed_extensions(&cmd)?;

        let resource_states = ctx.config.vscode_extensions.iter().map(|ext| {
            let resource = VsCodeExtensionResource::new(ext.id.clone(), cmd.clone());
            let state = resource.state_from_installed(&installed);
            (resource, state)
        });

        process_resource_states(
            ctx,
            resource_states,
            &ProcessOpts {
                verb: "install extension",
                fix_incorrect: false,
                fix_missing: true,
                bail_on_error: false,
            },
        )
    }
}
