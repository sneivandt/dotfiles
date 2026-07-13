//! Task: install the CLI wrapper script.
//!
//! Creates a small script in `~/.local/bin/` that delegates to the
//! repository's wrapper (`dotfiles.sh` or `dotfiles.ps1`), allowing the
//! user to run `dotfiles` from any directory.
//!
//! The wrapper type is chosen by the `DOTFILES_WRAPPER` environment
//! variable (set by the wrapper scripts themselves), falling back to
//! platform detection when the variable is absent.

use crate::domains::dotfiles::resources::wrapper::{WrapperResource, WrapperType};
use crate::engine::{
    Context, Domain, ProcessOpts, Task, TaskPhase, TaskResult, process_resources,
    process_resources_remove, task_metadata,
};

/// Install the CLI wrapper script in `~/.local/bin`.
#[derive(Debug)]
pub struct InstallWrapper;

impl Task for InstallWrapper {
    task_metadata! {
        name: "Install wrapper",
        phase: TaskPhase::Bootstrap,
        domain: Domain::Core,
    }

    fn run(&self, ctx: &Context) -> anyhow::Result<TaskResult> {
        let system = ctx.system();
        let paths = ctx.paths();
        let wrapper_type = WrapperType::detect(system.platform());
        let resource = WrapperResource::new(wrapper_type, paths.root(), paths.home());
        process_resources(
            ctx,
            std::iter::once(resource),
            &ProcessOpts::strict("install"),
        )
    }
}

/// Remove the CLI wrapper script from `~/.local/bin`.
#[derive(Debug)]
pub struct UninstallWrapper;

impl Task for UninstallWrapper {
    task_metadata! {
        name: "Remove wrapper",
        phase: TaskPhase::Bootstrap,
        domain: Domain::Core,
    }

    fn run(&self, ctx: &Context) -> anyhow::Result<TaskResult> {
        let system = ctx.system();
        let paths = ctx.paths();
        let wrapper_type = WrapperType::detect(system.platform());
        let resource = WrapperResource::new(wrapper_type, paths.root(), paths.home());
        process_resources_remove(ctx, std::iter::once(resource), "remove")
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
    use crate::runtime::platform::Os;
    use crate::test_helpers::{ContextBuilder, empty_config};
    use std::path::PathBuf;

    #[test]
    fn install_should_run_on_linux() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = ContextBuilder::new(config).os(Os::Linux).build();
        assert!(InstallWrapper.should_run(&ctx));
    }

    #[test]
    fn install_should_run_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = ContextBuilder::new(config).os(Os::Windows).build();
        assert!(InstallWrapper.should_run(&ctx));
    }

    #[test]
    fn uninstall_should_run() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = ContextBuilder::new(config).os(Os::Linux).build();
        assert!(UninstallWrapper.should_run(&ctx));
    }
}
