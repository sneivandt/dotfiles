use anyhow::Result;

use super::{Context, ProcessOpts, Task, TaskResult, process_resources};
use crate::resources::shell::DefaultShellResource;

/// Configure the default shell to zsh.
#[derive(Debug)]
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::platform::{Os, Platform};
    use crate::tasks::test_helpers::{NoOpExecutor, WhichExecutor, empty_config, make_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let platform = Platform::new(Os::Windows, false);
        let executor = WhichExecutor { which_result: true };
        let ctx = make_context(&config, &platform, &executor);
        assert!(!ConfigureShell.should_run(&ctx));
    }

    #[test]
    fn should_run_false_when_zsh_not_found() {
        let config = empty_config(PathBuf::from("/tmp"));
        let platform = Platform::new(Os::Linux, false);
        let executor = NoOpExecutor; // which() returns false
        let ctx = make_context(&config, &platform, &executor);
        assert!(!ConfigureShell.should_run(&ctx));
    }
}
