//! Task: configure Git settings.

use super::{ProcessOpts, resource_task};
use crate::resources::git_config::GitConfigResource;

resource_task! {
    /// Configure git settings from git-config.toml.
    pub ConfigureGit {
        name: "Configure git",
        items: |ctx| ctx.config_read().git_settings.clone(),
        build: |s, _ctx| GitConfigResource::new(s.key, s.value),
        opts: ProcessOpts::strict("set git config"),
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
