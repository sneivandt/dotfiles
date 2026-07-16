//! Task: ensure `~/.local/bin` is on the user's `PATH`.

use crate::domains::dotfiles::resources::path_entry::PathEntryResource;
use crate::engine::{
    Context, Domain, ProcessOpts, Task, TaskPhase, TaskResult, process_resources, task_metadata,
};

/// Ensure `~/.local/bin` is on the user's `PATH`.
#[derive(Debug)]
pub struct ConfigurePath;

impl Task for ConfigurePath {
    task_metadata! {
        name: "Configure PATH",
        phase: TaskPhase::Bootstrap,
        domain: Domain::Core,
        deps: [crate::domains::dotfiles::tasks::wrapper::InstallWrapper],
    }

    fn run(&self, ctx: &Context) -> anyhow::Result<TaskResult> {
        let system = ctx.system();
        let resource =
            PathEntryResource::new(system.home(), system.platform(), system.executor_arc());
        process_resources(
            ctx,
            std::iter::once(resource),
            &ProcessOpts::strict("configure"),
        )
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
    use crate::infra::platform::Os;
    use crate::test_helpers::{ContextBuilder, empty_config};
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
