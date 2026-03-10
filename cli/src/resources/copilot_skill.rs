//! GitHub Copilot plugin resource.
use anyhow::{Context as _, Result, bail};
use std::collections::HashSet;
use std::sync::Arc;

use super::{Applicable, ResourceChange, ResourceState};
use crate::exec::{self, Executor};

/// A GitHub Copilot plugin that can be checked and installed.
#[derive(Debug)]
pub struct CopilotSkillResource {
    /// Marketplace repository reference used with `gh copilot plugin marketplace add`.
    pub marketplace: String,
    /// Marketplace name used with `gh copilot plugin install <plugin>@<marketplace_name>`.
    pub marketplace_name: String,
    /// Plugin name to install from the marketplace.
    pub plugin: String,
    /// Executor for running Copilot CLI commands.
    executor: Arc<dyn Executor>,
}

/// Cached Copilot CLI state gathered from bulk list commands.
#[derive(Debug, Clone, Default)]
pub struct CopilotPluginCache {
    installed_plugins: HashSet<String>,
    registered_marketplaces: HashSet<String>,
}

impl CopilotPluginCache {
    /// Create an empty cache.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Determine whether a marketplace is already registered.
    #[must_use]
    pub fn is_marketplace_registered(&self, marketplace: &str, marketplace_name: &str) -> bool {
        self.registered_marketplaces
            .contains(&marketplace.to_lowercase())
            || self
                .registered_marketplaces
                .contains(&marketplace_name.to_lowercase())
    }
}

impl CopilotSkillResource {
    /// Create a new Copilot plugin resource.
    #[must_use]
    pub fn new(
        marketplace: String,
        marketplace_name: String,
        plugin: String,
        executor: Arc<dyn Executor>,
    ) -> Self {
        Self {
            marketplace,
            marketplace_name,
            plugin,
            executor,
        }
    }

    /// Create from a config entry.
    #[must_use]
    pub fn from_entry(
        entry: &crate::config::copilot_skills::CopilotSkill,
        executor: Arc<dyn Executor>,
    ) -> Self {
        Self::new(
            entry.marketplace.clone(),
            entry.marketplace_name.clone(),
            entry.plugin.clone(),
            executor,
        )
    }

    /// Determine the resource state from a pre-fetched Copilot CLI cache.
    #[must_use]
    pub fn state_from_cache(&self, cache: &CopilotPluginCache) -> ResourceState {
        if cache.installed_plugins.contains(&self.plugin_spec()) {
            ResourceState::Correct
        } else {
            ResourceState::Missing
        }
    }

    /// Return the requested plugin spec in normalized form.
    #[must_use]
    pub fn plugin_spec(&self) -> String {
        format!(
            "{}@{}",
            self.plugin.to_lowercase(),
            self.marketplace_name.to_lowercase()
        )
    }
}

impl Applicable for CopilotSkillResource {
    fn description(&self) -> String {
        format!("{}@{}", self.plugin, self.marketplace_name)
    }

    fn apply(&self) -> Result<ResourceChange> {
        install_plugin(&self.plugin, &self.marketplace_name, &*self.executor).with_context(
            || {
                format!(
                    "installing Copilot plugin {} from {}",
                    self.plugin, self.marketplace_name
                )
            },
        )?;

        Ok(ResourceChange::Applied)
    }
}

/// Query installed plugins and registered marketplaces in two bulk CLI calls.
///
/// # Errors
///
/// Returns an error if either Copilot CLI query fails.
pub fn get_copilot_plugin_state(executor: &dyn Executor) -> Result<CopilotPluginCache> {
    let installed = run_copilot_checked(
        &["copilot", "plugin", "list"],
        executor,
        "gh copilot plugin list",
    )?;
    let marketplaces = run_copilot_checked(
        &["copilot", "plugin", "marketplace", "list"],
        executor,
        "gh copilot plugin marketplace list",
    )?;

    Ok(CopilotPluginCache {
        installed_plugins: parse_installed_plugins(&installed.stdout),
        registered_marketplaces: parse_registered_marketplaces(&marketplaces.stdout),
    })
}

/// Register a marketplace with the Copilot CLI.
///
/// # Errors
///
/// Returns an error if marketplace registration fails.
pub fn register_marketplace(marketplace: &str, executor: &dyn Executor) -> Result<()> {
    run_copilot_checked(
        &["copilot", "plugin", "marketplace", "add", marketplace],
        executor,
        &format!("gh copilot plugin marketplace add {marketplace}"),
    )?;
    Ok(())
}

fn install_plugin(plugin: &str, marketplace_name: &str, executor: &dyn Executor) -> Result<()> {
    let spec = format!("{plugin}@{marketplace_name}");
    run_copilot_checked(
        &["copilot", "plugin", "install", &spec],
        executor,
        &format!("gh copilot plugin install {spec}"),
    )?;
    Ok(())
}

fn parse_installed_plugins(stdout: &str) -> HashSet<String> {
    stdout
        .lines()
        .filter_map(|line| {
            let item = trim_cli_item(line);
            let spec = item.split_whitespace().next()?;
            spec.contains('@').then(|| spec.to_lowercase())
        })
        .collect()
}

fn parse_registered_marketplaces(stdout: &str) -> HashSet<String> {
    let mut marketplaces = HashSet::new();

    for line in stdout.lines() {
        let item = trim_cli_item(line);
        let Some((name, repo_part)) = item.split_once(" (GitHub: ") else {
            continue;
        };
        let name = name.trim();
        let repo = repo_part.trim_end_matches(')').trim();
        if !name.is_empty() {
            marketplaces.insert(name.to_lowercase());
        }
        if !repo.is_empty() {
            marketplaces.insert(repo.to_lowercase());
        }
    }

    marketplaces
}

fn trim_cli_item(line: &str) -> &str {
    line.trim_start_matches(|ch: char| ch.is_whitespace() || matches!(ch, '•' | '◆' | '*' | '-'))
        .trim()
}

fn run_copilot_checked(
    args: &[&str],
    executor: &dyn Executor,
    label: &str,
) -> Result<exec::ExecResult> {
    let result = run_copilot_cmd(args, executor)?;
    if !result.success {
        let detail = if result.stderr.trim().is_empty() {
            result.stdout.trim()
        } else {
            result.stderr.trim()
        };
        bail!("{label} failed (exit {:?}): {}", result.code, detail);
    }
    Ok(result)
}

/// Run a GitHub CLI Copilot command.
fn run_copilot_cmd(args: &[&str], executor: &dyn Executor) -> Result<exec::ExecResult> {
    executor.run_unchecked("gh", args)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::exec::ExecResult;
    use std::collections::VecDeque;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    #[derive(Debug)]
    struct RecordingExecutor {
        responses: Mutex<VecDeque<ExecResult>>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl RecordingExecutor {
        fn new(responses: Vec<ExecResult>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn success(stdout: &str) -> ExecResult {
            ExecResult {
                stdout: stdout.to_string(),
                stderr: String::new(),
                success: true,
                code: Some(0),
            }
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }

        fn unexpected_error(message: &str) -> anyhow::Error {
            anyhow::anyhow!(message.to_string())
        }
    }

    impl Executor for RecordingExecutor {
        fn run(&self, _: &str, _: &[&str]) -> Result<ExecResult> {
            Err(Self::unexpected_error("unexpected checked executor call"))
        }

        fn run_in_with_env(
            &self,
            _: &Path,
            _: &str,
            _: &[&str],
            _: &[(&str, &str)],
        ) -> Result<ExecResult> {
            Err(Self::unexpected_error("unexpected run_in_with_env call"))
        }

        fn run_unchecked(&self, program: &str, args: &[&str]) -> Result<ExecResult> {
            self.calls.lock().unwrap().push(
                std::iter::once(program.to_string())
                    .chain(args.iter().map(|arg| (*arg).to_string()))
                    .collect(),
            );
            Ok(self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| ExecResult {
                    stdout: String::new(),
                    stderr: "unexpected call".to_string(),
                    success: false,
                    code: Some(1),
                }))
        }

        fn which(&self, _: &str) -> bool {
            true
        }

        fn which_path(&self, program: &str) -> Result<PathBuf> {
            Ok(PathBuf::from(format!("/usr/bin/{program}")))
        }
    }

    #[test]
    fn description_returns_plugin_reference() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = CopilotSkillResource::new(
            "dotnet/skills".to_string(),
            "dotnet-agent-skills".to_string(),
            "dotnet-diag".to_string(),
            Arc::clone(&executor),
        );
        assert_eq!(resource.description(), "dotnet-diag@dotnet-agent-skills");
    }

    #[test]
    fn state_from_cache_reports_correct_when_plugin_is_installed() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = CopilotSkillResource::new(
            "dotnet/skills".to_string(),
            "dotnet-agent-skills".to_string(),
            "dotnet-diag".to_string(),
            Arc::clone(&executor),
        );

        let cache = CopilotPluginCache {
            installed_plugins: HashSet::from(["dotnet-diag@dotnet-agent-skills".to_string()]),
            registered_marketplaces: HashSet::new(),
        };

        assert_eq!(resource.state_from_cache(&cache), ResourceState::Correct);
    }

    #[test]
    fn state_from_cache_reports_missing_when_plugin_is_absent() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = CopilotSkillResource::new(
            "dotnet/skills".to_string(),
            "dotnet-agent-skills".to_string(),
            "dotnet-diag".to_string(),
            Arc::clone(&executor),
        );

        let cache = CopilotPluginCache::empty();

        assert_eq!(resource.state_from_cache(&cache), ResourceState::Missing);
    }

    #[test]
    fn from_entry_copies_plugin_fields() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let entry = crate::config::copilot_skills::CopilotSkill {
            marketplace: "dotnet/skills".to_string(),
            marketplace_name: "dotnet-agent-skills".to_string(),
            plugin: "dotnet-msbuild".to_string(),
        };
        let resource = CopilotSkillResource::from_entry(&entry, Arc::clone(&executor));
        assert_eq!(resource.marketplace, "dotnet/skills");
        assert_eq!(resource.marketplace_name, "dotnet-agent-skills");
        assert_eq!(resource.plugin, "dotnet-msbuild");
    }

    #[test]
    fn parse_installed_plugins_reads_specs() {
        let installed = parse_installed_plugins(
            "Installed plugins:\n  • csharp-dotnet-development@awesome-copilot (v1.0.0)\n  • dotnet-diag@dotnet-agent-skills\n",
        );

        assert!(installed.contains("csharp-dotnet-development@awesome-copilot"));
        assert!(installed.contains("dotnet-diag@dotnet-agent-skills"));
    }

    #[test]
    fn parse_registered_marketplaces_reads_names_and_repos() {
        let marketplaces = parse_registered_marketplaces(
            "✨ Included with GitHub Copilot:\n  ◆ awesome-copilot (GitHub: github/awesome-copilot)\n\nRegistered marketplaces:\n  • dotnet-agent-skills (GitHub: dotnet/skills)\n",
        );

        assert!(marketplaces.contains("awesome-copilot"));
        assert!(marketplaces.contains("github/awesome-copilot"));
        assert!(marketplaces.contains("dotnet-agent-skills"));
        assert!(marketplaces.contains("dotnet/skills"));
    }

    #[test]
    fn get_copilot_plugin_state_reads_both_bulk_queries() {
        let executor = Arc::new(RecordingExecutor::new(vec![
            RecordingExecutor::success(
                "Installed plugins:\n  • dotnet-upgrade@dotnet-agent-skills\n",
            ),
            RecordingExecutor::success(
                "Registered marketplaces:\n  • dotnet-agent-skills (GitHub: dotnet/skills)\n",
            ),
        ]));
        let executor_trait: Arc<dyn Executor> = executor.clone();

        let cache = get_copilot_plugin_state(&*executor_trait).unwrap();

        let calls = executor.calls();
        assert_eq!(calls.len(), 2);
        assert!(calls[0].join(" ").contains("plugin list"));
        assert!(calls[1].join(" ").contains("plugin marketplace list"));
        assert!(
            cache
                .installed_plugins
                .contains("dotnet-upgrade@dotnet-agent-skills")
        );
        assert!(cache.is_marketplace_registered("dotnet/skills", "dotnet-agent-skills"));
    }

    #[test]
    fn register_marketplace_runs_add_command() {
        let executor = Arc::new(RecordingExecutor::new(vec![RecordingExecutor::success(
            "added\n",
        )]));
        let executor_trait: Arc<dyn Executor> = executor.clone();

        register_marketplace("dotnet/skills", &*executor_trait).unwrap();

        let calls = executor.calls();
        assert_eq!(calls.len(), 1);
        assert!(
            calls[0]
                .join(" ")
                .contains("plugin marketplace add dotnet/skills")
        );
    }

    #[test]
    fn apply_installs_plugin() {
        let executor = Arc::new(RecordingExecutor::new(vec![RecordingExecutor::success(
            "installed\n",
        )]));
        let executor_trait: Arc<dyn Executor> = executor.clone();
        let resource = CopilotSkillResource::new(
            "dotnet/skills".to_string(),
            "dotnet-agent-skills".to_string(),
            "dotnet-upgrade".to_string(),
            executor_trait,
        );

        assert!(matches!(resource.apply().unwrap(), ResourceChange::Applied));

        let calls = executor.calls();
        assert_eq!(calls.len(), 1);
        assert!(
            calls[0]
                .join(" ")
                .contains("plugin install dotnet-upgrade@dotnet-agent-skills")
        );
    }
}
