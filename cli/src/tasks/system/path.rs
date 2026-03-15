//! Task: ensure `~/.local/bin` is on the user's `PATH`.

use std::sync::Arc;

use crate::resources::path_entry::PathEntryResource;
use crate::tasks::{Context, ProcessOpts, Task, TaskPhase, TaskResult, process_resources};

/// Ensure `~/.local/bin` is on the user's `PATH`.
#[derive(Debug)]
pub struct ConfigurePath;

impl Task for ConfigurePath {
    fn name(&self) -> &'static str {
        "Configure PATH"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::System
    }

    crate::tasks::task_deps![crate::tasks::system::wrapper::InstallWrapper];

    fn should_run(&self, _ctx: &Context) -> bool {
        true
    }

    fn run(&self, ctx: &Context) -> anyhow::Result<TaskResult> {
        let resource = PathEntryResource::new(&ctx.home, &ctx.platform, Arc::clone(&ctx.executor));
        process_resources(
            ctx,
            std::iter::once(resource),
            &ProcessOpts::strict("configure PATH"),
        )
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::platform::Os;
    use crate::tasks::Task;
    use crate::tasks::test_helpers::{ContextBuilder, empty_config};
    use std::path::PathBuf;

    #[test]
    fn should_run_on_linux() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = ContextBuilder::new(config).os(Os::Linux).build();
        assert!(ConfigurePath.should_run(&ctx));
    }

    #[test]
    fn should_run_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = ContextBuilder::new(config).os(Os::Windows).build();
        assert!(ConfigurePath.should_run(&ctx));
    }

    #[test]
    fn depends_on_install_wrapper() {
        let deps = ConfigurePath.dependencies();
        assert!(
            !deps.is_empty(),
            "ConfigurePath should depend on InstallWrapper"
        );
    }
}
