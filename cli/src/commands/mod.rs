//! Top-level command handlers for install, uninstall, test, and version.
pub mod install;
mod scheduler;
pub mod test;
pub mod uninstall;
pub mod version;

use std::sync::Arc;

use anyhow::Result;

use crate::cli::GlobalOpts;
use crate::config::Config;
use crate::config::profiles;
use crate::logging::{Log, Logger};
use crate::platform::Platform;
use crate::tasks::{self, Context, Task};

/// Shared orchestration helper that combines setup and task execution.
///
/// Handles platform detection, profile resolution, config loading,
/// `Context` construction, and task execution in a single entry point.
#[derive(Debug)]
pub struct CommandRunner {
    ctx: Context,
    log: Arc<Logger>,
}

impl CommandRunner {
    /// Initialize by detecting the platform, resolving the profile, loading
    /// configuration, and building the task execution context.
    ///
    /// # Errors
    ///
    /// Returns an error if setup fails (profile resolution, configuration
    /// loading, or the HOME environment variable is not set).
    pub fn new(global: &GlobalOpts, log: &Arc<Logger>) -> Result<Self> {
        let platform = Platform::detect();
        let root = install::resolve_root(global)?;
        let profile = resolve_profile(global, &root, &platform, &**log)?;
        let config = load_config(&root, &profile, &platform, &**log)?;

        let executor: Arc<dyn crate::exec::Executor> = Arc::new(crate::exec::SystemExecutor);
        let ctx = Context::new(
            Arc::new(std::sync::RwLock::new(config)),
            Arc::new(platform),
            Arc::clone(log) as Arc<dyn Log>,
            executor,
            crate::tasks::ContextOpts {
                dry_run: global.dry_run,
                parallel: global.parallel,
            },
        )?;

        Ok(Self {
            ctx,
            log: Arc::clone(log),
        })
    }

    /// Execute the given tasks to completion using the stored context.
    ///
    /// # Errors
    ///
    /// Returns an error if one or more tasks fail.
    pub fn run<'a>(&self, tasks: impl IntoIterator<Item = &'a dyn Task>) -> Result<()> {
        run_tasks_to_completion(tasks, &self.ctx, &self.log)
    }
}

/// Resolve the active profile from CLI args, persisted git config, or an
/// interactive prompt, logging the result.
///
/// # Errors
///
/// Returns an error if the profile name is invalid, profile definitions cannot
/// be loaded from `profiles.toml`, or interactive prompting fails.
fn resolve_profile(
    global: &GlobalOpts,
    root: &std::path::Path,
    platform: &Platform,
    log: &dyn Log,
) -> Result<profiles::Profile> {
    log.stage("Resolving profile");
    let profile = profiles::resolve_from_args(global.profile.as_deref(), root, platform)?;
    log.info(&format!("profile: {}", profile.name));
    Ok(profile)
}

/// Load configuration for the given root, profile, and platform, emitting
/// debug counts and any validation warnings.
///
/// # Errors
///
/// Returns an error if any configuration file fails to parse.
fn load_config(
    root: &std::path::Path,
    profile: &profiles::Profile,
    platform: &Platform,
    log: &dyn Log,
) -> Result<Config> {
    log.stage("Loading configuration");
    let config = Config::load(root, profile, platform)?;

    macro_rules! debug_count {
        ($field:expr, $label:expr) => {
            log.debug(&format!("{} {}", $field.len(), $label));
        };
    }

    debug_count!(config.packages, "packages");
    debug_count!(config.symlinks, "symlinks");
    debug_count!(config.registry, "registry entries");
    debug_count!(config.units, "systemd units");
    debug_count!(config.chmod, "chmod entries");
    debug_count!(config.vscode_extensions, "vscode extensions");
    debug_count!(config.copilot_skills, "copilot skills");
    debug_count!(config.manifest.excluded_files, "manifest exclusions");
    log.info(&format!(
        "loaded {} packages, {} symlinks",
        config.packages.len(),
        config.symlinks.len()
    ));

    // Validate configuration and display warnings
    let warnings = config.validate(platform);
    if !warnings.is_empty() {
        log.warn(&format!(
            "found {} configuration warning(s):",
            warnings.len()
        ));
        for warning in &warnings {
            log.warn(&format!(
                "  {} [{}]: {}",
                warning.source, warning.item, warning.message
            ));
        }
    }

    Ok(config)
}

/// Execute every task respecting dependency order.
///
/// When parallel execution is enabled and more than one task is present,
/// tasks run as soon as their dependencies complete.  Each task's console
/// output is buffered and flushed atomically on completion.  A status line
/// shows which tasks are currently running.
///
/// When parallel execution is disabled (or only one task is present),
/// tasks execute sequentially in list order.
///
/// # Errors
///
/// Returns an error if one or more tasks recorded a failure.
pub fn run_tasks_to_completion<'a>(
    tasks: impl IntoIterator<Item = &'a dyn Task>,
    ctx: &Context,
    log: &Arc<Logger>,
) -> Result<()> {
    let tasks: Vec<&dyn Task> = tasks.into_iter().collect();

    if ctx.parallel && tasks.len() > 1 {
        if tasks::has_cycle(&tasks) {
            log.warn("dependency cycle detected; falling back to sequential execution");
            for task in &tasks {
                tasks::execute(*task, ctx);
            }
        } else {
            scheduler::run_tasks_parallel(&tasks, ctx, log);
        }
    } else {
        for task in &tasks {
            tasks::execute(*task, ctx);
        }
    }

    log.print_summary();

    let count = log.failure_count();
    if count > 0 {
        anyhow::bail!("{count} task(s) failed");
    }
    Ok(())
}
