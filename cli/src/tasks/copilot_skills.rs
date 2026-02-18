use anyhow::Result;

use super::{Context, ProcessOpts, Task, TaskResult, process_resources};
use crate::resources::copilot_skill::CopilotSkillResource;

/// Install GitHub Copilot skills.
pub struct InstallCopilotSkills;

impl Task for InstallCopilotSkills {
    fn name(&self) -> &'static str {
        "Install Copilot skills"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config.copilot_skills.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let skills_dir = ctx.home.join(".copilot/skills");
        ctx.log
            .debug(&format!("skills directory: {}", skills_dir.display()));

        let resources = ctx
            .config
            .copilot_skills
            .iter()
            .map(|skill| CopilotSkillResource::from_entry(skill, &skills_dir));
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
