//! Task: configure global git settings.

use crate::domains::git::config::git_config::GitSetting;
use crate::domains::git::resources::git_config::GitConfigResource;
use crate::engine::{ProcessOpts, config_resource_task};

config_resource_task! {
    /// Configure global git settings.
    pub ConfigureGit {
        name: "Configure Git",
        config: Vec<GitSetting>,
        items: |settings| settings.clone(),
        build: |setting, _ctx| GitConfigResource::new(setting.key, setting.value),
        opts: ProcessOpts::strict("configure").sequential(),
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
