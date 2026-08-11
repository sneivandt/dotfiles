//! Command startup composition and task-set construction.

use std::sync::Arc;

use anyhow::Result;

use crate::app::cli::GlobalOpts;
use crate::app::config::Config;
use crate::app::config::profiles;
use crate::app::config::store::ConfigStore;
use crate::engine::{Context, Task, TaskId};
use crate::infra::ConfigHandle;
use crate::infra::logging::{Log, LogEvent, Logger};
use crate::infra::platform::Platform;

use super::execution::{ExecutionPlan, RunCoordinator};
use crate::infra::logging::Output as _;
use crate::infra::logging::OutputExt as _;
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
        let root = resolve_root(global)?;
        let env = crate::infra::env::system();
        let overlay = crate::domains::overlay::resolution::resolve_from_args(
            global.overlay.as_deref(),
            &root,
            env.as_ref(),
        );
        let profile = resolve_profile(global, &root, platform, overlay.as_deref(), log)?;
        let config = load_config(&root, &profile, platform, overlay.as_deref(), log)?;
        let store = ConfigStore::from_config(config);

        let executor: Arc<dyn crate::infra::exec::Executor> =
            Arc::new(crate::infra::exec::ProcessExecutor::managed(token.clone()));
        let log_output: Arc<dyn Log> = Arc::<Logger>::clone(log);
        let ctx = Context::new(
            root,
            overlay.clone(),
            platform,
            log_output,
            executor,
            env,
            crate::engine::ContextOpts {
                dry_run: global.dry_run,
                parallel: global.parallel,
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
        RunCoordinator::new(&self.ctx, &self.log).execute(ExecutionPlan::single(tasks))
    }

    /// Execute tasks and inject additional tasks after a dependency boundary.
    ///
    /// # Errors
    ///
    /// Returns an error if graph validation fails or one or more tasks fail.
    pub fn run_with_late_tasks<'a>(
        &'a self,
        tasks: impl IntoIterator<Item = &'a dyn Task>,
        boundary: TaskId,
        provider: impl FnOnce() -> Vec<Box<dyn Task>> + 'a,
    ) -> Result<()> {
        RunCoordinator::new(&self.ctx, &self.log)
            .execute(ExecutionPlan::with_late_tasks(tasks, boundary, provider))
    }
}

/// Resolve the dotfiles root directory from CLI arguments or auto-detection.
///
/// # Errors
///
/// Returns an error if the root directory cannot be determined or doesn't exist.
pub(super) fn resolve_root(global: &GlobalOpts) -> Result<std::path::PathBuf> {
    let cwd = std::env::current_dir().ok();
    resolve_root_from_dir(global, cwd.as_deref())
}

fn resolve_root_from_dir(
    global: &GlobalOpts,
    cwd: Option<&std::path::Path>,
) -> Result<std::path::PathBuf> {
    if let Some(ref root) = global.root {
        return crate::infra::fs::canonicalize(root);
    }

    if let Ok(root) = std::env::var("DOTFILES_ROOT") {
        return Ok(std::path::PathBuf::from(root));
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        let candidates = [parent.join("../../.."), parent.join("..")];
        for candidate in &candidates {
            if candidate.join("conf").exists() && candidate.join("symlinks").exists() {
                return crate::infra::fs::canonicalize(candidate);
            }
        }
    }

    if let Some(cwd) = cwd
        && cwd.join("conf").exists()
        && cwd.join("symlinks").exists()
    {
        return crate::infra::fs::canonicalize(cwd);
    }

    anyhow::bail!("cannot determine dotfiles root. Use --root or set DOTFILES_ROOT env var");
}

fn resolve_profile(
    global: &GlobalOpts,
    root: &std::path::Path,
    platform: Platform,
    overlay: Option<&std::path::Path>,
    log: &Logger,
) -> Result<profiles::Profile> {
    // Run-log only: the startup header must be the first console line.
    log.run_event(LogEvent::Stage, "resolving profile");
    let profile = profiles::resolve_from_args(
        global.profile.as_deref(),
        root,
        platform,
        &crate::infra::env::SystemEnv,
    )?;
    log.startup(startup_context_line(
        &log.command_title(),
        &profile.name,
        platform,
        global.dry_run,
        overlay,
    ));
    // The header is metadata about the run, not part of it, so it always stands
    // apart from whatever follows — including a run where nothing had work to do
    // and the totals line is the only thing that follows.
    log.separate_from_startup();
    Ok(profile)
}

/// Build the single startup header line.
///
/// Sections are joined with ` · `; the overlay path, when one is active, is
/// the optional final section.
pub(super) fn startup_context_line(
    command_title: &str,
    profile_name: &str,
    platform: Platform,
    dry_run: bool,
    overlay: Option<&std::path::Path>,
) -> String {
    let mut platform_label = platform.description().to_string();
    if platform.is_wsl() {
        platform_label.push_str(" \u{00b7} WSL");
    }
    let dry_run = if dry_run { " \u{00b7} dry run" } else { "" };
    let overlay = overlay.map_or_else(String::new, |path| {
        format!(" \u{00b7} overlay {}", path.display())
    });
    format!(
        "{command_title}{dry_run} \u{00b7} profile {profile_name} \u{00b7} {platform_label}{overlay}"
    )
}

fn load_config(
    root: &std::path::Path,
    profile: &profiles::Profile,
    platform: Platform,
    overlay: Option<&std::path::Path>,
    log: &Logger,
) -> Result<Config> {
    tracing::debug!("loading configuration");
    let config = Config::load(root, profile, platform, overlay)?;

    // One line rather than nine: the counts are context for the run that
    // follows, and empty sections say nothing worth a row of their own.
    let sections: Vec<String> = config
        .section_counts()
        .iter()
        .filter(|section| section.count > 0)
        .map(|section| format!("{} {}", section.count, section.label()))
        .collect();
    log.debug(if sections.is_empty() {
        "Loaded configuration".to_string()
    } else {
        format!(
            "Loaded configuration \u{00b7} {}",
            sections.join(" \u{00b7} ")
        )
    });

    let warnings = config.validate(platform);
    crate::app::validation::display_diagnostics(&warnings, log);

    Ok(config)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod root_tests {
    use super::*;

    fn global(root: Option<std::path::PathBuf>) -> GlobalOpts {
        GlobalOpts {
            root,
            profile: None,
            dry_run: false,
            overlay: None,
            parallel: true,
            no_symbols: false,
            elevated_child: false,
        }
    }

    #[test]
    fn resolve_root_uses_explicit_root() {
        let temp_dir = tempfile::tempdir().unwrap();
        let result = resolve_root(&global(Some(temp_dir.path().to_path_buf()))).unwrap();
        assert_eq!(
            result,
            crate::infra::fs::canonicalize(temp_dir.path()).unwrap()
        );
    }

    #[test]
    fn resolve_root_canonicalizes_explicit_relative_root() {
        let temp_dir = tempfile::tempdir().unwrap();
        let result = resolve_root(&global(Some(temp_dir.path().join(".")))).unwrap();
        assert_eq!(
            result,
            crate::infra::fs::canonicalize(temp_dir.path()).unwrap()
        );
    }

    #[test]
    fn resolve_root_errors_when_not_in_repo() {
        let temp_dir = tempfile::tempdir().unwrap();
        if std::env::var("DOTFILES_ROOT").is_err() {
            let error = resolve_root_from_dir(&global(None), Some(temp_dir.path())).unwrap_err();
            assert!(error.to_string().contains("cannot determine dotfiles root"));
        }
    }
}
