//! Task: configure Git settings.
use anyhow::Result;

use super::{Context, ProcessOpts, Task, TaskResult, process_resources};
use crate::resources::git_config::GitConfigResource;

/// Configure git settings from git-config.toml.
#[derive(Debug)]
pub struct ConfigureGit;

impl Task for ConfigureGit {
    fn name(&self) -> &'static str {
        "Configure git"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config_read().git_settings.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let settings = ctx.config_read().git_settings.clone();
        let resources = settings
            .into_iter()
            .map(|s| GitConfigResource::new(s.key, s.value));
        process_resources(ctx, resources, &ProcessOpts::apply_all("set git config"))
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::git_config::GitSetting;
    use crate::tasks::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_when_no_settings() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(!ConfigureGit.should_run(&ctx));
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
