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

    /// Writing `HKLM\...\AppModelUnlock` is the one Windows mutation in the
    /// catalog that genuinely requires an administrator token.
    ///
    /// Returns `false` once the flag is set, so only the first install on a
    /// machine ever plans elevation.
    fn needs_elevation(&self, ctx: &Context) -> bool {
        ctx.platform().is_windows()
            && !ctx.system().is_elevated()
            && !crate::infra::platform::developer_mode_enabled()
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

    #[test]
    fn needs_elevation_false_on_linux() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(
            !EnableDeveloperMode.needs_elevation(&ctx),
            "developer mode is a Windows-only concept"
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn needs_elevation_true_on_windows_when_developer_mode_is_unset() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        // Off Windows the flag can never be observed as set and the process is
        // never elevated, so this exercises the first-run branch.
        assert!(
            EnableDeveloperMode.needs_elevation(&ctx),
            "an unset developer mode flag must plan elevation"
        );
    }
}
