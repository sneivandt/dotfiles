//! Task: install the CLI wrapper script.
//!
//! Creates a small script in `~/.local/bin/` that delegates to the
//! repository's wrapper (`dotfiles.sh` or `dotfiles.ps1`), allowing the
//! user to run `dotfiles` from any directory.
//!
//! The installed launcher format follows the target OS.

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
        let wrapper_type = WrapperType::detect(ctx.platform());
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
        let wrapper_type = WrapperType::detect(ctx.platform());
        let resource = WrapperResource::new(wrapper_type, ctx.root(), ctx.home());
        let mut resources = vec![resource];
        if wrapper_type == WrapperType::Sh {
            resources.push(WrapperResource::new(
                WrapperType::Pwsh,
                ctx.root(),
                ctx.home(),
            ));
        }
        process_resources_remove(ctx, resources, "remove")
    }
}
