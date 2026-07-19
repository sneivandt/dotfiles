//! Task: configure Git settings.
use anyhow::Result;
use std::path::PathBuf;

use crate::domains::git::config::git_config::GitSetting;
use crate::domains::git::resources::git_config::GitConfigResource;
use crate::engine::{
    Context, ProcessOpts, Task, TaskResult, configured_task_result, process_resources,
};
use crate::infra::ConfigHandle;

/// Configure git settings from git-config.toml.
#[derive(Debug)]
pub struct ConfigureGit {
    config: ConfigHandle<Vec<GitSetting>>,
    config_path: Option<PathBuf>,
}

impl ConfigureGit {
    /// Create the task using the user's global Git configuration.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<GitSetting>>) -> Self {
        Self {
            config,
            config_path: None,
        }
    }

    /// Create the task using one explicit Git configuration file.
    #[cfg(any(test, feature = "internal-api"))]
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

    fn run_resources(&self, ctx: &Context, emit_stage: bool) -> Result<Option<TaskResult>> {
        let settings = self.config.read();
        if settings.is_empty() {
            return Ok(None);
        }
        if emit_stage {
            ctx.log().task_stage(self.name());
        }

        let config_path = self.config_path.clone();
        let resources = settings.iter().cloned().map(|setting| {
            if let Some(path) = &config_path {
                GitConfigResource::with_config_path(setting.key, setting.value, path.clone())
            } else {
                GitConfigResource::new(setting.key, setting.value)
            }
        });
        process_resources(
            ctx,
            resources,
            &ProcessOpts::strict("configure").sequential(),
        )
        .map(Some)
    }
}

impl Task for ConfigureGit {
    fn name(&self) -> &'static str {
        "Configure Git"
    }

    fn run_configured(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        self.run_resources(ctx, true)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        Ok(configured_task_result(self.run_resources(ctx, false)?))
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
    use crate::domains::git::config::git_config::GitSetting;
    use crate::engine::Task;
    use crate::infra::ConfigHandle;
    use crate::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_is_true_without_explicit_guard() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(ConfigureGit::new(ConfigHandle::new(vec![])).should_run(&ctx));
    }

    #[test]
    fn should_run_true_with_settings() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        let task = ConfigureGit::new(ConfigHandle::new(vec![GitSetting {
            key: "core.autocrlf".to_string(),
            value: "false".to_string(),
        }]));
        assert!(task.should_run(&ctx));
    }
}
