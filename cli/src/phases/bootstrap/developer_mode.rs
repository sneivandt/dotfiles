//! Task: enable Windows Developer Mode.

use crate::phases::{ExecutionPolicy, ProcessOpts, TaskPhase, resource_task};
use crate::platform::Platform;
use crate::resources::developer_mode::DeveloperModeResource;

resource_task! {
    /// Enable Windows Developer Mode (allows symlink creation without admin).
    pub EnableDeveloperMode {
        name: "Enable developer mode",
        phase: TaskPhase::Bootstrap,
        policy: [ExecutionPolicy::PlatformSupported("Windows", Platform::is_windows)],
        guard: |ctx| ctx.platform.is_windows(),
        items: |_ctx| vec![()],
        build: |_unit, _ctx| DeveloperModeResource::new(),
        opts: ProcessOpts::lenient("enable"),
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
    use crate::phases::Task;
    use crate::phases::test_helpers::{empty_config, make_linux_context, make_windows_context};
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
