use anyhow::Result;

use super::{Context, ProcessOpts, Task, TaskResult, process_resources};
use crate::resources::shell::DefaultShellResource;

/// Configure the default shell to zsh.
pub struct ConfigureShell;

impl Task for ConfigureShell {
    fn name(&self) -> &'static str {
        "Configure default shell"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        // Skip in CI environments where chsh requires authentication
        let is_ci = std::env::var("CI").is_ok();
        ctx.platform.is_linux() && ctx.executor.which("zsh") && !is_ci
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resource = DefaultShellResource::new("zsh".to_string(), ctx.executor);
        process_resources(
            ctx,
            std::iter::once(resource),
            &ProcessOpts {
                verb: "configure",
                fix_incorrect: true,
                fix_missing: true,
                bail_on_error: true,
            },
        )
    }
}
