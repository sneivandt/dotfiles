//! Task: configure the login shell.

use anyhow::Result;

use crate::domains::shell::resources::shell::DefaultShellResource;
use crate::engine::{Context, ProcessOpts, Task, TaskResult, run_resource_task, task_metadata};

/// Configure the default shell to zsh.
#[derive(Debug)]
pub struct ConfigureShell;

const NAME: &str = "Default shell";

impl ConfigureShell {
    fn process(ctx: &Context, announce: Option<&'static str>) -> Result<TaskResult> {
        run_resource_task(
            ctx,
            announce,
            vec![()],
            |(), ctx| {
                DefaultShellResource::new(
                    "zsh".to_string(),
                    ctx.executor_arc(),
                    std::sync::Arc::clone(ctx.env()),
                )
            },
            &ProcessOpts::strict("configure"),
        )
    }
}

impl Task for ConfigureShell {
    task_metadata! {
        name: NAME,
        selector: "shell",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform().is_linux() && !ctx.is_ci()
    }

    fn run_configured(&self, ctx: &Context) -> Result<TaskResult> {
        if !ctx.which("zsh") {
            return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
        }
        Self::process(ctx, Some(NAME))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        Self::process(ctx, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Task;
    use crate::infra::platform::Os;
    use crate::test_helpers::{
        ContextBuilder, empty_config, make_linux_context, make_platform_context_with_which,
    };
    use std::path::PathBuf;

    #[test]
    fn should_run_false_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_platform_context_with_which(config, Os::Windows, false, true);
        assert!(!ConfigureShell.should_run(&ctx));
    }

    #[test]
    fn run_configured_is_not_applicable_when_zsh_is_still_unavailable() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config); // which() returns false
        assert!(ConfigureShell.should_run(&ctx));
        assert!(matches!(
            ConfigureShell.run_configured(&ctx).unwrap(),
            TaskResult::NotApplicable(_)
        ));
    }

    #[test]
    fn should_run_false_when_ci() {
        let config = empty_config(PathBuf::from("/tmp"));
        // Use ContextBuilder.ci(true) — no env var mutation required.
        let ctx = ContextBuilder::new(config)
            .os(Os::Linux)
            .which(true)
            .ci(true)
            .build();
        assert!(
            !ConfigureShell.should_run(&ctx),
            "should not configure shell in CI"
        );
    }

    #[test]
    fn should_run_true_on_linux_with_zsh_outside_ci() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = ContextBuilder::new(config)
            .os(Os::Linux)
            .which(true)
            .ci(false)
            .build();
        assert!(
            ConfigureShell.should_run(&ctx),
            "should configure shell on Linux when zsh is available and not in CI"
        );
    }
}
