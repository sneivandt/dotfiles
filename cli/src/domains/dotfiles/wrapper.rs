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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::exec::MockExecutor;
    use crate::infra::logging::Logger;
    use crate::infra::platform::{Os, Platform};
    use crate::test_helpers::assert_task_changed;
    use std::sync::Arc;

    #[test]
    fn uninstall_wrapper_on_linux_removes_both_sh_and_pwsh_wrappers() {
        let root = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let ctx = Context::from_raw(
            root.path().to_path_buf(),
            None,
            Platform::new(Os::Linux, false),
            Arc::new(Logger::test()),
            Arc::new(MockExecutor::new()),
            home.path().to_path_buf(),
            crate::engine::ContextOpts::default(),
        );
        let sh = WrapperResource::new(WrapperType::Sh, ctx.root(), ctx.home());
        let pwsh = WrapperResource::new(WrapperType::Pwsh, ctx.root(), ctx.home());
        crate::engine::Resource::apply(&sh).unwrap();
        crate::engine::Resource::apply(&pwsh).unwrap();
        let sh_path = home.path().join(".local/bin/dotfiles");
        let pwsh_path = home.path().join(".local/bin/dotfiles.cmd");
        assert!(sh_path.exists() && pwsh_path.exists());

        let result = UninstallWrapper.run(&ctx).unwrap();
        assert_task_changed(&result);
        assert!(!sh_path.exists());
        assert!(!pwsh_path.exists());
    }
}
