//! Task: configure Git settings.

use anyhow::Result;

use super::{Context, ProcessOpts, Task, TaskResult, run_resource_task};
use crate::resources::git_config::GitConfigResource;

/// Configure git settings from git-config.toml.
#[derive(Debug)]
pub struct ConfigureGit;

impl Task for ConfigureGit {
    fn name(&self) -> &'static str {
        "Configure Git"
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        true
    }

    fn run_if_applicable(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        let items: Vec<_> = ctx.config_read().git_settings.clone();
        if items.is_empty() {
            return Ok(None);
        }
        run_resource_task(
            ctx,
            items,
            |s, _ctx| GitConfigResource::new(s.key, s.value),
            &ProcessOpts::strict("set git config").sequential(),
        )
        .map(Some)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let items: Vec<_> = ctx.config_read().git_settings.clone();
        if items.is_empty() {
            return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
        }
        run_resource_task(
            ctx,
            items,
            |s, _ctx| GitConfigResource::new(s.key, s.value),
            &ProcessOpts::strict("set git config").sequential(),
        )
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::git_config::GitSetting;
    use crate::tasks::Task;
    use crate::tasks::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_is_true_without_explicit_guard() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(ConfigureGit.should_run(&ctx));
    }

    #[test]
    fn should_run_true_with_settings() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.git_settings.push(GitSetting {
            key: "core.autocrlf".to_string(),
            value: "false".to_string(),
        });
        let ctx = make_linux_context(config);
        assert!(ConfigureGit.should_run(&ctx));
    }
}
