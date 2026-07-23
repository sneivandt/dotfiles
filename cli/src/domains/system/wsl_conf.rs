//! Task: configure WSL settings.

use anyhow::Result;

use crate::domains::system::resources::wsl_conf::WslConfResource;
use crate::engine::{
    Context, IntrinsicState, ProcessOpts, ResourceState, Task, TaskResult, process_resources,
    task_metadata,
};

/// Ensure `/etc/wsl.conf` enables systemd and disables Windows PATH injection.
#[derive(Debug)]
pub struct InstallWslConf;

impl Task for InstallWslConf {
    task_metadata! {
        name: "Configure WSL",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform().is_wsl() && !ctx.system().is_ci()
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        let resource = WslConfResource::system(ctx.system().executor_arc());
        !matches!(resource.current_state(), Ok(ResourceState::Correct))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_resources(
            ctx,
            [WslConfResource::system(ctx.system().executor_arc())],
            &ProcessOpts::strict("configure").sequential(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::platform::Os;
    use crate::test_helpers::{ContextBuilder, empty_config};
    use std::path::PathBuf;

    #[test]
    fn runs_only_in_wsl_outside_ci() {
        assert!(
            InstallWslConf.should_run(
                &ContextBuilder::new(empty_config(PathBuf::from("/tmp")))
                    .os(Os::Linux)
                    .wsl(true)
                    .build()
            )
        );
        assert!(
            !InstallWslConf.should_run(
                &ContextBuilder::new(empty_config(PathBuf::from("/tmp")))
                    .os(Os::Linux)
                    .wsl(false)
                    .build()
            )
        );
        assert!(
            !InstallWslConf.should_run(
                &ContextBuilder::new(empty_config(PathBuf::from("/tmp")))
                    .os(Os::Linux)
                    .wsl(true)
                    .ci(true)
                    .build()
            )
        );
    }
}
