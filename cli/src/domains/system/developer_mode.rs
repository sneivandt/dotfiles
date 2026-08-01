//! Task: enable Windows Developer Mode.

use anyhow::Result;

use crate::domains::system::resources::developer_mode::DeveloperModeResource;
use crate::engine::{
    Context, ProcessOpts, Task, TaskResult, configured_task_result, run_resource_task,
    task_metadata,
};

/// Enable Windows Developer Mode (allows symlink creation without admin).
#[derive(Debug)]
pub struct EnableDeveloperMode;

const NAME: &str = "Windows Developer Mode";

impl EnableDeveloperMode {
    fn process(ctx: &Context, announce: Option<&'static str>) -> Result<Option<TaskResult>> {
        run_resource_task(
            ctx,
            announce,
            vec![()],
            |(), _ctx| DeveloperModeResource::new(),
            &ProcessOpts::lenient("enable"),
        )
    }
}

impl Task for EnableDeveloperMode {
    task_metadata! {
        name: NAME,
        selector: "developer-mode",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform().is_windows()
    }

    fn run_configured(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        Self::process(ctx, Some(NAME))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        Ok(configured_task_result(Self::process(ctx, None)?))
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
    use crate::engine::Task;
    use crate::test_helpers::{empty_config, make_linux_context, make_windows_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_on_linux() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(!EnableDeveloperMode.should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        assert!(EnableDeveloperMode.should_run(&ctx));
    }
}
