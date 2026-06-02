//! Task: configure GitHub Copilot CLI settings.

use crate::resources::copilot_settings::CopilotSettingResource;
use crate::tasks::{Domain, ProcessOpts, TaskPhase, resource_task};

resource_task! {
    /// Configure Copilot CLI settings from copilot.toml.
    ///
    /// Each managed key is converged inside `~/.copilot/settings.json` without
    /// disturbing keys the Copilot CLI manages itself.  Processing is forced
    /// sequential because every resource reads and rewrites the same file.
    pub ConfigureCopilot {
        name: "Configure Copilot",
        phase: TaskPhase::Apply,
        domain: Domain::Ai,
        items: |ctx| ctx.config_read().copilot_settings.clone(),
        build: |s, ctx| CopilotSettingResource::new(
            s.key.clone(),
            s.json_value(),
            ctx.home.join(".copilot").join("settings.json"),
        ),
        opts: ProcessOpts::strict("set copilot setting").sequential(),
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
    use crate::config::copilot::CopilotSetting;
    use crate::tasks::Task;
    use crate::tasks::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_is_true_without_explicit_guard() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(ConfigureCopilot.should_run(&ctx));
    }

    #[test]
    fn run_is_not_applicable_without_settings() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        let result = ConfigureCopilot.run(&ctx).unwrap();
        assert!(matches!(result, crate::tasks::TaskResult::NotApplicable(_)));
    }

    #[test]
    fn run_with_settings_converges() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = empty_config(dir.path().to_path_buf());
        config.copilot_settings.push(CopilotSetting {
            key: "model".to_string(),
            value: toml::Value::String("claude-opus-4.8".to_string()),
        });
        let ctx = make_linux_context(config).with_home(dir.path().to_path_buf());
        let _result = ConfigureCopilot.run(&ctx).unwrap();

        let written =
            std::fs::read_to_string(dir.path().join(".copilot").join("settings.json")).unwrap();
        assert!(written.contains("claude-opus-4.8"));
    }
}
