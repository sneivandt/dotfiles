use anyhow::Result;

use super::{Context, Task, TaskResult};

/// Install GitHub Copilot skills.
pub struct InstallCopilotSkills;

impl Task for InstallCopilotSkills {
    fn name(&self) -> &str {
        "Install Copilot skills"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config.copilot_skills.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let skills_dir = ctx.home.join(".copilot/skills");

        if ctx.dry_run {
            for skill in &ctx.config.copilot_skills {
                ctx.log.dry_run(&format!("install skill: {}", skill.url));
            }
            return Ok(TaskResult::DryRun);
        }

        if !skills_dir.exists() {
            std::fs::create_dir_all(&skills_dir)?;
        }

        let mut count = 0u32;
        for skill in &ctx.config.copilot_skills {
            ctx.log.debug(&format!("skill: {}", skill.url));
            count += 1;
        }

        ctx.log.info(&format!("{count} skills configured"));
        Ok(TaskResult::Ok)
    }
}
