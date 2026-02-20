use anyhow::Result;

use super::{Context, ProcessOpts, Task, TaskResult, process_resources};
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

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resource = DeveloperModeResource::new();
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::platform::{Os, Platform};
    use crate::tasks::test_helpers::{NoOpExecutor, empty_config, make_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_on_linux() {
        let config = empty_config(PathBuf::from("/tmp"));
        let platform = Platform::new(Os::Linux, false);
        let executor = NoOpExecutor;
        let ctx = make_context(config, &platform, &executor);
        assert!(!EnableDeveloperMode.should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let platform = Platform::new(Os::Windows, false);
        let executor = NoOpExecutor;
        let ctx = make_context(config, &platform, &executor);
        assert!(EnableDeveloperMode.should_run(&ctx));
    }
}
