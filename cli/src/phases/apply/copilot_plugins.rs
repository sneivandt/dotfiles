//! Task: install GitHub Copilot plugins.

use anyhow::Result;
use std::collections::BTreeSet;
use std::sync::Arc;

use crate::phases::{Context, ProcessOpts, Task, TaskPhase, TaskResult, process_resource_states};
use crate::resources::ResourceState;
use crate::resources::copilot_plugin::{
    CopilotPluginCache, CopilotPluginResource, copilot_supports_plugins, get_copilot_plugin_state,
    get_copilot_version, register_marketplace,
};

/// Install GitHub Copilot plugins.
#[derive(Debug)]
pub struct InstallCopilotPlugins;

impl Task for InstallCopilotPlugins {
    fn name(&self) -> &'static str {
        "Install Copilot plugins"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Apply
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config_read().copilot_plugins.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let gh_available = ctx.executor.which("gh");
        if !ctx.dry_run && !gh_available {
            ctx.log.debug("gh CLI not found in PATH");
            return Ok(TaskResult::Skipped("gh CLI not found".to_string()));
        }

        // Log the extension version if available.
        if gh_available {
            match get_copilot_version(&*ctx.executor) {
                Ok(version) => ctx.debug_fmt(|| {
                    format!(
                        "Copilot extension version: {}.{}.{}",
                        version.0, version.1, version.2
                    )
                }),
                Err(e) => ctx.log.debug(&format!(
                    "could not determine Copilot extension version: {e}"
                )),
            }
        }

        // Probe whether the `plugin` subcommand is available.
        let plugins_supported = if gh_available {
            copilot_supports_plugins(&*ctx.executor)
        } else {
            false
        };

        if !plugins_supported && !ctx.dry_run {
            return Ok(TaskResult::Skipped(
                "Copilot CLI does not support plugin subcommands".to_string(),
            ));
        }
        if !plugins_supported {
            ctx.log.debug(
                "Copilot plugin commands unavailable; assuming plugins are missing for dry-run",
            );
        }

        let plugins: Vec<_> = ctx.config_read().copilot_plugins.clone();
        let cache = if plugins_supported {
            ctx.debug_fmt(|| format!("batch-checking {} Copilot plugins", plugins.len()));
            match get_copilot_plugin_state(&*ctx.executor) {
                Ok(c) => c,
                Err(e) if ctx.dry_run => {
                    ctx.log.debug(&format!(
                        "could not fetch plugin cache (dry-run): {e}; assuming plugins are missing"
                    ));
                    CopilotPluginCache::empty()
                }
                Err(e) => return Err(e),
            }
        } else {
            CopilotPluginCache::empty()
        };

        let mut missing_marketplaces = BTreeSet::new();
        let resource_states: Vec<_> = plugins
            .into_iter()
            .map(|plugin| {
                let resource =
                    CopilotPluginResource::from_entry(&plugin, Arc::clone(&ctx.executor));
                let state = if plugins_supported {
                    resource.state_from_cache(&cache)
                } else {
                    ResourceState::Missing
                };
                if matches!(state, ResourceState::Missing)
                    && !cache
                        .is_marketplace_registered(&plugin.marketplace, &plugin.marketplace_name)
                {
                    missing_marketplaces.insert((plugin.marketplace, plugin.marketplace_name));
                }
                (resource, state)
            })
            .collect();

        if !ctx.dry_run {
            for (marketplace, marketplace_name) in missing_marketplaces {
                ctx.debug_fmt(|| {
                    format!("registering Copilot marketplace {marketplace_name} ({marketplace})")
                });
                register_marketplace(&marketplace, &*ctx.executor)?;
            }
        }

        process_resource_states(
            ctx,
            resource_states,
            &ProcessOpts::install_missing("install plugin").sequential(),
        )
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::copilot_plugins::CopilotPlugin;
    use crate::phases::Task;
    use crate::phases::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::exec::{ExecResult, MockExecutor};
    use crate::phases::test_helpers::make_context;
    use crate::platform::{Os, Platform};

    #[test]
    fn should_run_is_false_without_plugins() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(!InstallCopilotPlugins.should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_plugins_configured() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.copilot_plugins.push(CopilotPlugin {
            marketplace: "dotnet/skills".to_string(),
            marketplace_name: "dotnet-agent-skills".to_string(),
            plugin: "dotnet-diag".to_string(),
        });
        let ctx = make_linux_context(config);
        assert!(InstallCopilotPlugins.should_run(&ctx));
    }

    #[test]
    fn run_skips_when_gh_cli_not_found() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.copilot_plugins.push(CopilotPlugin {
            marketplace: "dotnet/skills".to_string(),
            marketplace_name: "dotnet-agent-skills".to_string(),
            plugin: "dotnet-msbuild".to_string(),
        });
        let ctx = make_linux_context(config);
        let result = InstallCopilotPlugins.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Skipped(ref s) if s.contains("gh CLI not found")),
            "expected 'gh CLI not found' skip, got {result:?}"
        );
    }

    #[test]
    fn run_skips_when_plugin_subcommand_unavailable() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.copilot_plugins.push(CopilotPlugin {
            marketplace: "dotnet/skills".to_string(),
            marketplace_name: "dotnet-agent-skills".to_string(),
            plugin: "dotnet-msbuild".to_string(),
        });
        let mut executor = MockExecutor::new();
        executor.expect_which().returning(|_| true);
        // First call: gh extension list (version check)
        executor.expect_run_unchecked().times(1).returning(|_, args| {
            if args.contains(&"extension") {
                Ok(ExecResult {
                    stdout: "NAME        REPO               VERSION\ngh copilot  github/gh-copilot  v1.2.0\n".to_string(),
                    stderr: String::new(),
                    success: true,
                    code: Some(0),
                })
            } else {
                // Second call: gh copilot plugin list (probe)
                Ok(ExecResult {
                    stdout: String::new(),
                    stderr: "error: Invalid command format.\n\nDid you mean: copilot -i \"plugin list\"?".to_string(),
                    success: false,
                    code: Some(1),
                })
            }
        });
        executor.expect_run_unchecked().times(1).returning(|_, _| {
            Ok(ExecResult {
                stdout: String::new(),
                stderr:
                    "error: Invalid command format.\n\nDid you mean: copilot -i \"plugin list\"?"
                        .to_string(),
                success: false,
                code: Some(1),
            })
        });
        let executor = Arc::new(executor);
        let platform = Platform {
            os: Os::Linux,
            is_arch: false,
            is_wsl: false,
        };
        let ctx = make_context(config, platform, executor);
        let result = InstallCopilotPlugins.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Skipped(ref s) if s.contains("does not support plugin")),
            "expected plugin support skip, got {result:?}"
        );
    }
}
