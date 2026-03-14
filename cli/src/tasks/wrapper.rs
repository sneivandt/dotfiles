//! Task: install the CLI wrapper script.
//!
//! Creates a small script in `~/.local/bin/` that delegates to the
//! repository's wrapper (`dotfiles.sh` or `dotfiles.ps1`), allowing the
//! user to run `dotfiles` from any directory.
//!
//! The wrapper type is chosen by the `DOTFILES_WRAPPER` environment
//! variable (set by the wrapper scripts themselves), falling back to
//! platform detection when the variable is absent.

use super::{Context, ProcessOpts, Task, TaskResult, process_resources, process_resources_remove};
use crate::resources::wrapper::{WrapperResource, WrapperType};

/// Install the CLI wrapper script in `~/.local/bin`.
#[derive(Debug)]
pub struct InstallWrapper;

impl Task for InstallWrapper {
    fn name(&self) -> &'static str {
        "Install wrapper"
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        true
    }

    fn run(&self, ctx: &Context) -> anyhow::Result<TaskResult> {
        let wrapper_type = WrapperType::detect(&ctx.platform);
        let resource = WrapperResource::new(wrapper_type, &ctx.root(), &ctx.home);
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
    fn name(&self) -> &'static str {
        "Uninstall wrapper"
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        true
    }

    fn run(&self, ctx: &Context) -> anyhow::Result<TaskResult> {
        let wrapper_type = WrapperType::detect(&ctx.platform);
        let resource = WrapperResource::new(wrapper_type, &ctx.root(), &ctx.home);
        process_resources_remove(ctx, std::iter::once(resource), "remove")
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
