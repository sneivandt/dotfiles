use anyhow::Result;

use super::{Context, Task, TaskResult, TaskStats};
use crate::resources::copilot_skill::CopilotSkillResource;
use crate::resources::{Resource, ResourceChange, ResourceState};

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

        let mut stats = TaskStats::new();

        for skill in &ctx.config.copilot_skills {
            let resource = CopilotSkillResource::from_entry(skill, &skills_dir);

            match resource.current_state()? {
                ResourceState::Correct => {
                    ctx.log.debug(&format!(
                        "ok: {} (already installed)",
                        resource.description()
                    ));
                    stats.already_ok += 1;
                }
                ResourceState::Missing => {
                    if ctx.dry_run {
                        ctx.log
                            .dry_run(&format!("would install skill: {}", resource.description()));
                        stats.changed += 1;
                        continue;
                    }

                    match resource.apply() {
                        Ok(ResourceChange::Applied) => {
                            ctx.log
                                .debug(&format!("installed skill: {}", resource.description()));
                            stats.changed += 1;
                        }
                        Ok(ResourceChange::Skipped { reason }) => {
                            ctx.log.warn(&format!(
                                "skipped skill {}: {reason}",
                                resource.description()
                            ));
                            stats.skipped += 1;
                        }
                        Ok(ResourceChange::AlreadyCorrect) => {
                            stats.already_ok += 1;
                        }
                        Err(e) => {
                            ctx.log.warn(&format!(
                                "failed to install skill: {}: {e}",
                                resource.description()
                            ));
                        }
                    }
                }
                ResourceState::Incorrect { current } => {
                    ctx.log.debug(&format!(
                        "skill {} unexpected state: {current}",
                        resource.description()
                    ));
                    stats.skipped += 1;
                }
                ResourceState::Invalid { reason } => {
                    ctx.log
                        .debug(&format!("skipping {}: {reason}", resource.description()));
                    stats.skipped += 1;
                }
            }
        }

        Ok(stats.finish(ctx))
    }
}
