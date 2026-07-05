//! Task: install VS Code extensions.
use anyhow::Result;
use std::collections::HashSet;
use std::sync::Arc;

use crate::resources::vscode_extension::{
    VsCodeExtensionResource, find_code_command, get_installed_extensions,
};
use crate::tasks::{
    Context, Domain, ProcessOpts, Task, TaskPhase, TaskResult,
    process_resources_with_borrowed_cache, task_metadata,
};

/// Install VS Code extensions.
#[derive(Debug)]
pub struct InstallVsCodeExtensions;

impl Task for InstallVsCodeExtensions {
    task_metadata! {
        name: "Install VS Code extensions",
        phase: TaskPhase::Provision,
        domain: Domain::Editors,
        deps: [
            crate::tasks::packages::InstallPackages,
            crate::tasks::packages::InstallAurPackages,
        ],
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config_read().vscode_extensions.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let Some(cmd) = find_code_command(&*ctx.executor) else {
            ctx.log
                .debug("neither code-insiders nor code found in PATH");
            return Ok(TaskResult::Skipped("VS Code CLI not found".to_string()));
        };

        ctx.debug_fmt(|| format!("using VS Code CLI: {cmd}"));
        let extensions: Vec<_> = ctx.config_read().vscode_extensions.clone();
        ctx.debug_fmt(|| {
            format!(
                "batch-checking {} extensions with a single query",
                extensions.len()
            )
        });
        let installed = get_installed_extensions(&cmd, &*ctx.executor)?;

        let resources = extensions.iter().map(|ext| {
            VsCodeExtensionResource::new(ext.id.clone(), cmd.clone(), Arc::clone(&ctx.executor))
        });
        process_resources_with_borrowed_cache(
            ctx,
            resources,
            &installed,
            |resource: &VsCodeExtensionResource, installed: &HashSet<String>| {
                Ok(resource.state_from_installed(installed))
            },
            &ProcessOpts::install_missing("install"),
        )
    }
}
