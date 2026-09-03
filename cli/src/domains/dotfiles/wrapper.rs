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
    Context, ProcessOpts, Task, TaskResult, process_resources, process_resources_remove,
    task_metadata,
};

/// Install the CLI wrapper script in `~/.local/bin`.
#[derive(Debug)]
pub struct InstallWrapper;

impl Task for InstallWrapper {
    task_metadata! {
        name: "Dotfiles launcher",
        selector: "launcher",
    }

    fn run(&self, ctx: &Context) -> anyhow::Result<TaskResult> {
        let wrapper_type = WrapperType::detect(ctx.env().as_ref(), ctx.platform());
        let resource = WrapperResource::new(wrapper_type, ctx.root(), ctx.home());
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
        name: "Dotfiles launcher",
        selector: "launcher",
    }

    fn run(&self, ctx: &Context) -> anyhow::Result<TaskResult> {
        let wrapper_type = WrapperType::detect(ctx.env().as_ref(), ctx.platform());
        let resource = WrapperResource::new(wrapper_type, ctx.root(), ctx.home());
        process_resources_remove(ctx, std::iter::once(resource), "remove")
    }
}
