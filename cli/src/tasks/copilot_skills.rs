//! Task: install GitHub Copilot skills.

use super::{ProcessOpts, resource_task};
use crate::resources::copilot_skill::CopilotSkillResource;

resource_task! {
    /// Install GitHub Copilot skills.
    pub InstallCopilotSkills {
        name: "Install Copilot skills",
        deps: [super::symlinks::InstallSymlinks],
        items: |ctx| ctx.config_read().copilot_skills.clone(),
        build: |skill, ctx| {
            use std::sync::Arc;
            let skills_dir = ctx.home.join(".copilot/skills");
            CopilotSkillResource::from_entry(
                &skill,
                &skills_dir,
                Arc::clone(&ctx.executor),
            )
        },
        opts: ProcessOpts::install_missing("install skill"),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::copilot_skills::CopilotSkill;
    use crate::tasks::Task;
    use crate::tasks::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    #[test]
    fn run_skips_when_no_skills_configured() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        // empty items cause should_run() to return false
        assert!(!InstallCopilotSkills.should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_skills_configured() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.copilot_skills.push(CopilotSkill {
            url: "https://github.com/example/skill".to_string(),
        });
        let ctx = make_linux_context(config);
        assert!(InstallCopilotSkills.should_run(&ctx));
    }
}
