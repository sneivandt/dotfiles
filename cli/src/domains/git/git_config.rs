//! Task: configure global git settings.

use anyhow::Result;

use crate::domains::git::config::git_config::GitSetting;
use crate::domains::git::resources::git_config::GitConfigResource;
use crate::engine::{
    Context, ProcessOpts, Task, TaskResult, configured_task_result, run_resource_task,
    task_metadata,
};
use crate::infra::ConfigHandle;

/// Configure global git settings.
#[derive(Debug)]
pub struct ConfigureGit {
    config: ConfigHandle<Vec<GitSetting>>,
}

const NAME: &str = "Git settings";

impl ConfigureGit {
    /// Create the task with a handle to its configuration slice.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<GitSetting>>) -> Self {
        Self { config }
    }

    fn process(&self, ctx: &Context, announce: Option<&'static str>) -> Result<Option<TaskResult>> {
        let settings = self.config.read().to_vec();
        run_resource_task(
            ctx,
            announce,
            settings,
            |setting, _ctx| GitConfigResource::new(setting.key, setting.value),
            &ProcessOpts::strict("configure").sequential(),
        )
    }
}

impl Task for ConfigureGit {
    task_metadata! {
        name: NAME,
        selector: "git",
    }

    fn run_configured(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        self.process(ctx, Some(NAME))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        Ok(configured_task_result(self.process(ctx, None)?))
    }
}
