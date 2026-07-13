//! Task: configure Git settings.

use crate::domains::git::config::git_config::GitSetting;
use crate::domains::git::resources::git_config::GitConfigResource;
use crate::engine::{Domain, ProcessOpts, TaskPhase, config_resource_task};

config_resource_task! {
    /// Configure git settings from git-config.toml.
    pub ConfigureGit {
        name: "Configure Git",
        phase: TaskPhase::Provision,
        domain: Domain::Git,
        config: Vec<GitSetting>,
        items: |cfg| cfg.clone(),
        build: |s, _ctx| GitConfigResource::new(s.key, s.value),
        opts: ProcessOpts::strict("configure").sequential(),
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
    use crate::runtime::ConfigHandle;
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
