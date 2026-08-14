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
        let manages_autocrlf = settings
            .iter()
            .any(|setting| setting.key.eq_ignore_ascii_case("core.autocrlf"));
        let mut resources = settings
            .into_iter()
            .map(|setting| GitConfigResource::new(setting.key, setting.value))
            .collect::<Vec<_>>();

        if ctx.platform().is_windows() && !manages_autocrlf {
            resources.push(GitConfigResource::absent("core.autocrlf".to_string()));
        }

        run_resource_task(
            ctx,
            announce,
            resources,
            |resource, _ctx| resource,
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
