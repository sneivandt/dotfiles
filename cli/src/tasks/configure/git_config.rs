//! Task: configure Git settings.

use crate::resources::git_config::GitConfigResource;
use crate::tasks::{ProcessOpts, TaskPhase, resource_task};

resource_task! {
    /// Configure git settings from git-config.toml.
    pub ConfigureGit {
        name: "Configure Git",
        phase: TaskPhase::Configure,
        items: |ctx| ctx.config_read().git_settings.clone(),
        build: |s, _ctx| GitConfigResource::new(s.key, s.value),
        opts: ProcessOpts::strict("set git config").sequential(),
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
