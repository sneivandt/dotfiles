use anyhow::Result;

use super::{Context, ProcessOpts, Task, TaskResult, process_resources};
use crate::resources::developer_mode::DeveloperModeResource;

/// Enable Windows Developer Mode (allows symlink creation without admin).
pub struct EnableDeveloperMode;

impl Task for EnableDeveloperMode {
    fn name(&self) -> &'static str {
        "Enable developer mode"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_windows()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resource = DeveloperModeResource::new(ctx.executor);
        process_resources(
            ctx,
            std::iter::once(resource),
            &ProcessOpts {
                verb: "enable",
                fix_incorrect: true,
                fix_missing: true,
                bail_on_error: false,
            },
        )
    }
}
