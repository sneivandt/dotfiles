//! Task: enable Windows Developer Mode.

use crate::domains::system::resources::developer_mode::DeveloperModeResource;
use crate::engine::{Domain, PlatformCapability, ProcessOpts, TaskPhase, resource_task};

resource_task! {
    /// Enable Windows Developer Mode (allows symlink creation without admin).
    pub EnableDeveloperMode {
        name: "Enable developer mode",
        phase: TaskPhase::Bootstrap,
        domain: Domain::System,
        policy: [PlatformCapability::Windows.policy()],
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
