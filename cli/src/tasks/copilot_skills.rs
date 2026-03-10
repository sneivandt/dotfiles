//! Task: install GitHub Copilot skills.

use anyhow::Result;
use std::collections::BTreeSet;
use std::sync::Arc;

use super::{Context, ProcessOpts, Task, TaskResult, process_resource_states, task_deps};
use crate::resources::ResourceState;
use crate::resources::copilot_skill::{
    CopilotPluginCache, CopilotSkillResource, get_copilot_plugin_state, register_marketplace,
};

/// Install GitHub Copilot skills.
#[derive(Debug)]
pub struct InstallCopilotSkills;

impl Task for InstallCopilotSkills {
    fn name(&self) -> &'static str {
        "Install Copilot skills"
    }

    task_deps![super::reload_config::ReloadConfig];

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config_read().copilot_skills.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let gh_available = ctx.executor.which("gh");
        if !ctx.dry_run && !gh_available {
            ctx.log.debug("gh CLI not found in PATH");
            return Ok(TaskResult::Skipped("gh CLI not found".to_string()));
        }

        let skills: Vec<_> = ctx.config_read().copilot_skills.clone();
        let cache = if gh_available {
            ctx.log.debug(&format!(
                "batch-checking {} Copilot plugins with a single CLI query",
                skills.len()
            ));
            get_copilot_plugin_state(&*ctx.executor)?
        } else {
            ctx.log
                .debug("gh CLI not found in PATH; assuming plugins are missing for dry-run");
            CopilotPluginCache::empty()
        };

        let mut missing_marketplaces = BTreeSet::new();
        let resource_states: Vec<_> = skills
            .into_iter()
            .map(|skill| {
                let resource = CopilotSkillResource::from_entry(&skill, Arc::clone(&ctx.executor));
                let state = if gh_available {
                    resource.state_from_cache(&cache)
                } else {
                    ResourceState::Missing
                };
                if matches!(state, ResourceState::Missing)
                    && !cache.is_marketplace_registered(&skill.marketplace, &skill.marketplace_name)
                {
                    missing_marketplaces.insert((skill.marketplace, skill.marketplace_name));
                }
                (resource, state)
            })
            .collect();

        if !ctx.dry_run {
            for (marketplace, marketplace_name) in missing_marketplaces {
                ctx.log.debug(&format!(
                    "registering Copilot marketplace {marketplace_name} ({marketplace})"
                ));
                register_marketplace(&marketplace, &*ctx.executor)?;
            }
        }

        process_resource_states(
            ctx,
            resource_states,
            &ProcessOpts::install_missing("install skill").sequential(),
        )
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::copilot_skills::CopilotSkill;
    use crate::tasks::Task;
    use crate::tasks::test_helpers::{empty_config, make_linux_context};
    use std::any::TypeId;
    use std::path::PathBuf;

    #[test]
    fn should_run_is_false_without_skills() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(!InstallCopilotSkills.should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_skills_configured() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.copilot_skills.push(CopilotSkill {
            marketplace: "dotnet/skills".to_string(),
            marketplace_name: "dotnet-agent-skills".to_string(),
            plugin: "dotnet-diag".to_string(),
        });
        let ctx = make_linux_context(config);
        assert!(InstallCopilotSkills.should_run(&ctx));
    }

    #[test]
    fn run_skips_when_gh_cli_not_found() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.copilot_skills.push(CopilotSkill {
            marketplace: "dotnet/skills".to_string(),
            marketplace_name: "dotnet-agent-skills".to_string(),
            plugin: "dotnet-msbuild".to_string(),
        });
        let ctx = make_linux_context(config);
        let result = InstallCopilotSkills.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Skipped(ref s) if s.contains("gh CLI not found")),
            "expected 'gh CLI not found' skip, got {result:?}"
        );
    }

    #[test]
    fn depends_on_reload_config() {
        assert_eq!(
            InstallCopilotSkills.dependencies(),
            &[TypeId::of::<crate::tasks::reload_config::ReloadConfig>()]
        );
    }
}
