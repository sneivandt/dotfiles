//! Task: enable Windows Developer Mode.

use anyhow::Result;

use super::{Context, ProcessOpts, Task, TaskResult, run_resource_task};
use crate::resources::developer_mode::DeveloperModeResource;

/// Enable Windows Developer Mode (allows symlink creation without admin).
#[derive(Debug)]
pub struct EnableDeveloperMode;

impl Task for EnableDeveloperMode {
    fn name(&self) -> &'static str {
        "Enable developer mode"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_windows()
    }

    fn run_if_applicable(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        if !ctx.platform.is_windows() {
            return Ok(None);
        }
        run_resource_task(
            ctx,
            vec![()],
            |_unit, _ctx| DeveloperModeResource::new(),
            &ProcessOpts::lenient("enable"),
        )
        .map(Some)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        run_resource_task(
            ctx,
            vec![()],
            |_unit, _ctx| DeveloperModeResource::new(),
            &ProcessOpts::lenient("enable"),
        )
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::tasks::Task;
    use crate::tasks::test_helpers::{empty_config, make_linux_context, make_windows_context};
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
