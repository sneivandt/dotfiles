//! Task: install VS Code extensions.
use crate::domains::editors::resources::vscode_extension::{
    VsCodeExtensionResource, find_code_command, get_installed_extensions,
};
use crate::engine::{
    Context, ProcessOpts, Task, TaskResult, process_resources_with_borrowed_cache,
};
use crate::infra::ConfigHandle;
use anyhow::Result;
use std::collections::HashSet;

/// Install VS Code extensions.
#[derive(Debug)]
pub struct InstallVsCodeExtensions {
    config: ConfigHandle<Vec<String>>,
}

impl InstallVsCodeExtensions {
    /// Create the task with a handle to the extension configuration.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<String>>) -> Self {
        Self { config }
    }
}

impl Task for InstallVsCodeExtensions {
    fn name(&self) -> &'static str {
        "Install VS Code extensions"
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        !self.config.read().is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let system = ctx.system();
        let Some(cmd) = find_code_command(system.executor()) else {
            ctx.log().debug("no VS Code CLI launcher found in PATH");
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
        let installed = get_installed_extensions(&cmd, system.executor())?;

        let resources = extensions
            .iter()
            .map(|id| VsCodeExtensionResource::new(id.clone(), cmd.clone(), system.executor_arc()));
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

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::infra::ConfigHandle;
    use crate::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    fn ext() -> String {
        "github.copilot".to_string()
    }

    #[test]
    fn should_run_false_when_no_extensions_configured() {
        let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));
        assert!(!InstallVsCodeExtensions::new(ConfigHandle::new(vec![])).should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_extensions_configured() {
        let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));
        let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![ext()]));
        assert!(task.should_run(&ctx));
    }

    #[test]
    fn run_skips_when_vscode_cli_not_found() {
        // Default make_linux_context uses TestExecutor with which_result=false,
        // so find_code_command returns None for both "code-insiders" and "code".
        let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));
        let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![ext()]));
        let result = task.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Skipped(ref s) if s.contains("VS Code CLI not found")),
            "expected 'VS Code CLI not found' skip, got {result:?}"
        );
    }
}
