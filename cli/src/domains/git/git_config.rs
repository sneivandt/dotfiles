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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Task;
    use crate::infra::ConfigHandle;
    use crate::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    fn setting(key: &str, value: &str) -> GitSetting {
        GitSetting {
            key: key.to_string(),
            value: value.to_string(),
        }
    }

    #[test]
    fn should_run_with_settings() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        let task = ConfigureGit::new(ConfigHandle::new(vec![setting("user.name", "Test User")]));
        assert!(task.should_run(&ctx));
    }

    #[test]
    fn should_run_without_settings() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(ConfigureGit::new(ConfigHandle::new(vec![])).should_run(&ctx));
    }
}
