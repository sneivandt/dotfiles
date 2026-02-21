use anyhow::Result;
use std::any::TypeId;

use super::{Context, ProcessOpts, Task, TaskResult, process_resources};
use crate::resources::copilot_skill::CopilotSkillResource;

/// Install GitHub Copilot skills.
#[derive(Debug)]
pub struct InstallCopilotSkills;

impl Task for InstallCopilotSkills {
    fn name(&self) -> &'static str {
        "Install Copilot skills"
    }

    fn dependencies(&self) -> &[TypeId] {
        const DEPS: &[TypeId] = &[TypeId::of::<super::symlinks::InstallSymlinks>()];
        DEPS
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config_read().copilot_skills.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let skills: Vec<_> = ctx.config_read().copilot_skills.clone();
        let skills_dir = ctx.home.join(".copilot/skills");
        ctx.log
            .debug(&format!("skills directory: {}", skills_dir.display()));

        let resources = skills
            .iter()
            .map(|skill| CopilotSkillResource::from_entry(skill, &skills_dir, &*ctx.executor));
        process_resources(
            ctx,
            resources,
            &ProcessOpts {
                verb: "install skill",
                fix_incorrect: false,
                fix_missing: true,
                bail_on_error: false,
            },
        )
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::copilot_skills::CopilotSkill;
    use crate::tasks::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_when_no_skills_configured() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
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
