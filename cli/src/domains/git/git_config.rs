//! Task: configure global git settings.

use anyhow::Result;
use std::path::PathBuf;

use crate::domains::git::config::git_config::GitSetting;
use crate::domains::git::resources::git_config::GitConfigResource;
use crate::engine::{Context, ProcessOpts, Task, TaskResult, run_resource_task, task_metadata};
use crate::infra::ConfigHandle;

/// Configure global git settings.
#[derive(Debug)]
pub struct ConfigureGit {
    config: ConfigHandle<Vec<GitSetting>>,
    config_path: Option<PathBuf>,
}

const NAME: &str = "Git settings";

impl ConfigureGit {
    /// Create the task with a handle to its configuration slice.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<GitSetting>>) -> Self {
        Self {
            config,
            config_path: None,
        }
    }

    /// Create the task with all settings scoped to one explicit config file.
    #[must_use]
    pub const fn with_config_path(
        config: ConfigHandle<Vec<GitSetting>>,
        config_path: PathBuf,
    ) -> Self {
        Self {
            config,
            config_path: Some(config_path),
        }
    }
}

impl Task for ConfigureGit {
    task_metadata! {
        name: NAME,
        selector: "git",
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let settings = self.config.get();
        let manages_autocrlf = settings
            .iter()
            .any(|setting| setting.key.eq_ignore_ascii_case("core.autocrlf"));
        let autocrlf_cleanup = (ctx.platform().is_windows() && !manages_autocrlf)
            .then(|| GitConfigResource::absent("core.autocrlf".to_string()));
        let resources = settings
            .iter()
            .map(|setting| GitConfigResource::new(setting.key.clone(), setting.value.clone()))
            .chain(autocrlf_cleanup)
            .map(|resource| {
                if let Some(path) = &self.config_path {
                    resource.using_config_path(path.clone())
                } else {
                    resource
                }
            })
            .collect();
        run_resource_task(
            ctx,
            resources,
            &ProcessOpts::strict("configure").sequential(),
        )
    }
}
