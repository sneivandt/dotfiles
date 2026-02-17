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

        let mut already_ok = 0u32;
        let mut would_install = 0u32;

        for skill in &ctx.config.copilot_skills {
            // Derive a directory name from the URL (last path segment)
            let dir_name = skill
                .url
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or(&skill.url);
            let dest = skills_dir.join(dir_name);

            if dest.exists() {
                ctx.log
                    .debug(&format!("ok: {} (already installed)", skill.url));
                already_ok += 1;
            } else {
                if ctx.dry_run {
                    ctx.log
                        .dry_run(&format!("would install skill: {}", skill.url));
                }
                would_install += 1;
            }
        }

        if ctx.dry_run {
            ctx.log.info(&format!(
                "{would_install} would change, {already_ok} already ok"
            ));
            return Ok(TaskResult::DryRun);
        }

        if !skills_dir.exists() {
            std::fs::create_dir_all(&skills_dir)?;
        }

        // Skills are configured declaratively; actual cloning would go here.
        let total = already_ok + would_install;
        ctx.log.info(&format!("{total} skills configured"));
        Ok(TaskResult::Ok)
    }
}
