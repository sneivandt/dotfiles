//! Command startup composition and task-set construction.

use std::sync::Arc;

use anyhow::Result;

use crate::app::cli::GlobalOpts;
use crate::app::config::Config;
use crate::app::config::profiles;
use crate::app::config::store::ConfigStore;
use crate::engine::{Context, Task, TaskPhase};
use crate::infra::ConfigHandle;
use crate::infra::logging::{Log, Logger, Output};
use crate::infra::platform::Platform;

use super::execution::{run_tasks_to_completion, run_tasks_to_completion_with_late_tasks};
use super::install;

/// Shared orchestration helper that combines setup and task execution.
#[derive(Debug)]
pub struct CommandRunner {
    ctx: Context,
    log: Arc<Logger>,
    store: ConfigStore,
    overlay: Option<std::path::PathBuf>,
}

impl CommandRunner {
    /// Initialize application configuration and the task execution context.
    ///
    /// # Errors
    ///
    /// Returns an error if profile resolution, configuration loading, or
    /// context construction fails.
    pub fn new(
        global: &GlobalOpts,
        log: &Arc<Logger>,
        token: &crate::engine::CancellationToken,
    ) -> Result<Self> {
        let platform = Platform::detect();
        let root = install::resolve_root(global)?;
        let profile = resolve_profile(global, &root, platform, log)?;
        let overlay = resolve_overlay(global, &root, &**log);
        if log.is_verbose() {
            log.separate_from_startup();
        }
        let config = load_config(&root, &profile, platform, overlay.as_deref(), log)?;
        let store = ConfigStore::from_config(config);

        let executor: Arc<dyn crate::infra::exec::Executor> =
            Arc::new(crate::infra::exec::ManagedExecutor::new(token.clone()));
        let log_output: Arc<dyn Log> = Arc::<Logger>::clone(log);
        let ctx = Context::new(
            root,
            overlay.clone(),
            platform,
            log_output,
            executor,
            crate::engine::ContextOpts {
                dry_run: global.dry_run,
                parallel: global.parallel,
                advance_versions: false,
                is_ci: None,
            },
        )?
        .with_cancellation(token.clone());

        Ok(Self {
            ctx,
            log: Arc::clone(log),
            store,
            overlay,
        })
    }

    /// Configure command-specific pipeline behavior.
    #[must_use]
    pub(crate) fn with_run_mode(mut self, mode: install::RunMode) -> Self {
        self.ctx = self.ctx.with_advance_versions(mode.advances_versions());
        self
    }

    /// Build the full set of install tasks, wired to the shared config store.
    #[must_use]
    pub fn install_tasks(&self) -> Vec<Box<dyn Task>> {
        crate::app::catalog::all_install_tasks(self.store.clone())
    }

    /// Build the full set of uninstall tasks, wired to the shared config store.
    #[must_use]
    pub fn uninstall_tasks(&self) -> Vec<Box<dyn Task>> {
        crate::app::catalog::all_uninstall_tasks(&self.store)
    }

    /// A handle to the aggregate configuration for app-owned validation tasks.
    #[must_use]
    pub fn config_handle(&self) -> ConfigHandle<Config> {
        self.store.aggregate.clone()
    }

    /// Create dynamic overlay script tasks from the current configuration.
    #[must_use]
    pub fn overlay_script_tasks(&self) -> Vec<Box<dyn Task>> {
        self.overlay.as_ref().map_or_else(Vec::new, |root| {
            let scripts = self.store.scripts.read();
            crate::domains::overlay::scripts::overlay_script_tasks(&scripts, root)
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

    /// Execute tasks and inject additional tasks after a phase completes.
    ///
    /// # Errors
    ///
    /// Returns an error if graph validation fails or one or more tasks fail.
    pub fn run_with_late_tasks<'a>(
        &'a self,
        tasks: impl IntoIterator<Item = &'a dyn Task>,
        after_phase: TaskPhase,
        provider: impl FnOnce() -> Vec<Box<dyn Task>> + 'a,
    ) -> Result<()> {
        run_tasks_to_completion_with_late_tasks(tasks, &self.ctx, &self.log, after_phase, provider)
    }
}

fn resolve_profile(
    global: &GlobalOpts,
    root: &std::path::Path,
    platform: Platform,
    log: &Logger,
) -> Result<profiles::Profile> {
    log.debug("Resolving profile");
    let profile = profiles::resolve_from_args(global.profile.as_deref(), root, platform)?;
    log.always(&startup_context_line(
        log.command_title(),
        &profile.name,
        platform,
        global.dry_run,
    ));
    Ok(profile)
}

pub(super) fn startup_context_line(
    command_title: &str,
    profile_name: &str,
    platform: Platform,
    dry_run: bool,
) -> String {
    let mut platform_label = platform.description().to_string();
    if platform.is_wsl() {
        platform_label.push_str(" \u{00b7} WSL");
    }
    let preview = if dry_run { " \u{00b7} preview" } else { "" };
    format!("{command_title} \u{00b7} profile {profile_name} \u{00b7} {platform_label}{preview}")
}

fn resolve_overlay(
    global: &GlobalOpts,
    root: &std::path::Path,
    log: &dyn Output,
) -> Option<std::path::PathBuf> {
    let overlay =
        crate::domains::overlay::resolution::resolve_from_args(global.overlay.as_deref(), root);
    log_overlay_path(overlay.as_deref(), log);
    overlay
}

pub(super) fn log_overlay_path(overlay: Option<&std::path::Path>, log: &dyn Output) {
    if let Some(path) = overlay {
        log.always(&format!("\x1b[2moverlay\x1b[0m {}", path.display()));
    }
}

fn load_config(
    root: &std::path::Path,
    profile: &profiles::Profile,
    platform: Platform,
    overlay: Option<&std::path::Path>,
    log: &Logger,
) -> Result<Config> {
    log.debug("Loading configuration");
    let config = Config::load(root, profile, platform, overlay)?;

    for section in config.section_counts() {
        log.debug(&format!("{} {}", section.count, section.label));
    }

    let warnings = config.validate(platform);
    if !warnings.is_empty() && !log.is_verbose() {
        log.separate_from_startup();
    }
    crate::infra::config::validation::display_diagnostics(&warnings, log);

    Ok(config)
}
