use anyhow::Result;
use std::any::TypeId;

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

    fn dependencies(&self) -> &[TypeId] {
        const DEPS: &[TypeId] = &[TypeId::of::<super::reload_config::ReloadConfig>()];
        DEPS
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config_read().vscode_extensions.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let Some(cmd) = find_code_command(ctx.executor) else {
            ctx.log
                .debug("neither code-insiders nor code found in PATH");
            return Ok(TaskResult::Skipped("VS Code CLI not found".to_string()));
        };

        ctx.log.debug(&format!("using VS Code CLI: {cmd}"));
        let extensions: Vec<_> = ctx.config_read().vscode_extensions.clone();
        ctx.log.debug(&format!(
            "batch-checking {} extensions with a single query",
            extensions.len()
        ));
        let installed = get_installed_extensions(&cmd, ctx.executor)?;

        process_resource_states(
            ctx,
            extensions.iter().map(|ext| {
                let resource =
                    VsCodeExtensionResource::new(ext.id.clone(), cmd.clone(), ctx.executor);
                let state = resource.state_from_installed(&installed);
                (resource, state)
            }),
            &ProcessOpts {
                verb: "install extension",
                fix_incorrect: false,
                fix_missing: true,
                bail_on_error: false,
            },
        )
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::vscode_extensions::VsCodeExtension;
    use crate::platform::{Os, Platform};
    use crate::tasks::test_helpers::{NoOpExecutor, empty_config, make_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_when_no_extensions_configured() {
        let config = empty_config(PathBuf::from("/tmp"));
        let platform = Platform::new(Os::Linux, false);
        let executor = NoOpExecutor;
        let ctx = make_context(config, &platform, &executor);
        assert!(!InstallVsCodeExtensions.should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_extensions_configured() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.vscode_extensions.push(VsCodeExtension {
            id: "github.copilot".to_string(),
        });
        let platform = Platform::new(Os::Linux, false);
        let executor = NoOpExecutor;
        let ctx = make_context(config, &platform, &executor);
        assert!(InstallVsCodeExtensions.should_run(&ctx));
    }
}
