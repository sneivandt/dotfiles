//! Task: install VS Code extensions.
use anyhow::Result;
use std::collections::HashSet;
use std::sync::Arc;

use crate::domains::editors::config::vscode_extensions::VsCodeExtension;
use crate::domains::editors::resources::vscode_extension::{
    VsCodeExtensionResource, find_code_command, get_installed_extensions,
};
use crate::engine::{
    Context, Domain, ProcessOpts, Task, TaskPhase, TaskResult,
    process_resources_with_borrowed_cache,
};
use crate::runtime::ConfigHandle;

/// Install VS Code extensions.
#[derive(Debug)]
pub struct InstallVsCodeExtensions {
    config: ConfigHandle<Vec<VsCodeExtension>>,
}

impl InstallVsCodeExtensions {
    /// Create the task with a handle to the extension configuration.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<VsCodeExtension>>) -> Self {
        Self { config }
    }
}

impl Task for InstallVsCodeExtensions {
    fn name(&self) -> &'static str {
        "Install VS Code extensions"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Provision
    }

    fn domain(&self) -> Domain {
        Domain::Editors
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        !self.config.read().is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let Some(cmd) = find_code_command(&*ctx.executor) else {
            ctx.log.debug("no VS Code CLI launcher found in PATH");
            return Ok(TaskResult::Skipped("VS Code CLI not found".to_string()));
        };

        ctx.debug_fmt(|| format!("using VS Code CLI: {cmd}"));
        let extensions: Vec<_> = self.config.read().to_vec();
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
