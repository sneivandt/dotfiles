//! Task: enable Windows Developer Mode.

use super::{ProcessOpts, resource_task};
use crate::resources::developer_mode::DeveloperModeResource;

resource_task! {
    /// Enable Windows Developer Mode (allows symlink creation without admin).
    pub EnableDeveloperMode {
        name: "Enable developer mode",
        guard: |ctx| ctx.platform.is_windows(),
        items: |_ctx| vec![()],
        build: |_unit, _ctx| DeveloperModeResource::new(),
        opts: ProcessOpts::lenient("enable"),
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
