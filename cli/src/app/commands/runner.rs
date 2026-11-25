//! Command startup composition and task-set construction.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;

use crate::app::config::Config;
use crate::app::config::profiles;
use crate::app::config::store::ConfigStore;
use crate::domains::overlay::resolution::{self as overlay_resolution, OverlaySource};
use crate::domains::repository::update::{CheckoutSnapshot, ensure_recovery_clear};
use crate::engine::{Context, Task, TaskId};
use crate::infra::ConfigHandle;
use crate::infra::logging::{Log, LogEvent, Logger};
use crate::infra::platform::Platform;

use super::RuntimePolicy;
use super::execution::{BoundaryAction, ExecutionPlan, RunCoordinator};
use crate::infra::logging::Output as _;
use crate::infra::logging::OutputExt as _;

/// Keep the accepted checkout selections and their command lock through startup.
pub(super) struct CommandStartup {
    root: PathBuf,
    overlay: Option<(PathBuf, OverlaySource)>,
    run_lock: Option<crate::infra::run_lock::CommandLocks>,
}

impl CommandStartup {
    fn resolve(runtime: &RuntimePolicy<'_>, log: &Logger) -> Result<Self> {
        let root = resolve_root(runtime)?;
        let run_lock = CommandRunner::acquire_run_lock(runtime, &root, log)?;
        let overlay = if runtime.repository_child && !runtime.execution.elevated_child {
            let path = runtime
                .env
                .var_os(super::reexec::REPOSITORY_OVERLAY_VAR)
                .ok_or_else(|| {
                    anyhow::anyhow!("repository restart is missing accepted overlay; retry update")
                })?;
            (!path.is_empty()).then(|| (PathBuf::from(path), OverlaySource::Environment))
        } else {
            overlay_resolution::resolve_from_args(
                runtime.global.overlay.as_deref(),
                &root,
                runtime.env.as_ref(),
                runtime.execution.non_interactive,
            )?
        };
        Ok(Self {
            root,
            overlay,
            run_lock,
        })
    }

    pub(super) fn for_mutation(runtime: &RuntimePolicy<'_>, log: &Logger) -> Result<Self> {
        let mut startup = Self::resolve(runtime, log)?;
        // Task filters control synchronization, not permission to consume a partial checkout.
        ensure_recovery_clear(&startup.root)?;
        // The preflight can restart before the overlay is needed. Keep the
        // recovery check before that restart, then check again with the lock.
        if super::reexec::should_check_self_update(runtime) {
            startup.ensure_overlay_recovery_clear()?;
            super::reexec::prepare_self_update(
                runtime,
                &startup.root,
                startup.run_lock.as_ref(),
                log,
            )?;
        }
        startup.protect_overlay(runtime, log)?;
        startup.ensure_overlay_recovery_clear()?;
        Ok(startup)
    }

    fn ensure_overlay_recovery_clear(&self) -> Result<()> {
        if let Some((overlay, _)) = &self.overlay {
            ensure_recovery_clear(overlay)?;
        }
        Ok(())
    }

    fn report_checkout_recovery(&self, log: &Logger) -> Result<bool> {
        use crate::domains::repository::update::pending_recovery_marker;

        let mut pending = false;
        for root in std::iter::once(self.root.as_path())
            .chain(self.overlay.as_ref().map(|(path, _)| path.as_path()))
        {
            if let Some(marker) = pending_recovery_marker(root)? {
                log.warn(format!(
                    "Unverified repository merge in {}. Inspect Git status and diffs before clearing the recovery marker {}",
                    root.display(),
                    marker.display()
                ));
                pending = true;
            }
        }
        Ok(pending)
    }

    pub(super) fn checkout_snapshot(&self) -> Result<CheckoutSnapshot> {
        CheckoutSnapshot::capture(
            &self.root,
            self.overlay.as_ref().map(|(path, _)| path.as_path()),
        )
    }

    fn protect_overlay(&mut self, runtime: &RuntimePolicy<'_>, log: &Logger) -> Result<()> {
        if let (Some(locks), Some((overlay, _))) = (&mut self.run_lock, &self.overlay) {
            use super::reexec::{REEXEC_GUARD_VAR, REPOSITORY_OVERLAY_LOCK_VAR};
            let marker = runtime.env.var_os(REEXEC_GUARD_VAR);
            let inherited_marker = inherited_overlay_marker(
                runtime.repository_child,
                runtime.env.is_set(REPOSITORY_OVERLAY_LOCK_VAR),
                marker.as_deref(),
            );
            locks.protect_overlay(
                overlay,
                runtime.env.as_ref(),
                Platform::detect(),
                &log.command_title(),
                inherited_marker.map(|value| (self.root.as_path(), value)),
            )?;
        }
        Ok(())
    }
}

fn inherited_overlay_marker(
    repository_child: bool,
    overlay_lock_set: bool,
    marker: Option<&std::ffi::OsStr>,
) -> Option<&std::ffi::OsStr> {
    if !repository_child || !overlay_lock_set {
        return None;
    }
    marker.filter(|marker| !marker.is_empty())
}

/// Shared orchestration helper that combines setup and task execution.
#[derive(Debug)]
pub(crate) struct CommandRunner {
    run_lock: Option<crate::infra::run_lock::CommandLocks>,
    ctx: Context,
    log: Arc<Logger>,
    store: ConfigStore,
}

impl CommandRunner {
    /// Initialize application configuration and the task execution context.
    ///
    /// # Errors
    ///
    /// Returns an error if profile resolution, configuration loading, or
    /// context construction fails.
    pub(super) fn for_check(
        runtime: &RuntimePolicy<'_>,
        log: &Arc<Logger>,
        token: &crate::engine::CancellationToken,
    ) -> Result<(Self, bool)> {
        let mut startup = CommandStartup::resolve(runtime, log)?;
        startup.protect_overlay(runtime, log)?;
        let recovery_pending = startup.report_checkout_recovery(log)?;
        let runner = Self::from_startup(runtime, log, token, startup)?;
        Ok((runner, recovery_pending))
    }

    fn acquire_run_lock(
        runtime: &RuntimePolicy<'_>,
        root: &Path,
        log: &Logger,
    ) -> Result<Option<crate::infra::run_lock::CommandLocks>> {
        if runtime.execution.elevated_child {
            return Ok(None);
        }

        let platform = Platform::detect();
        let home = crate::engine::context::home_directory(runtime.env.as_ref(), platform)?;
        if let Some(marker) = runtime
            .env
            .var_os(super::reexec::REEXEC_GUARD_VAR)
            .filter(|marker| !marker.is_empty())
        {
            return crate::infra::run_lock::CommandLocks::restart(
                root,
                &home,
                runtime.env.as_ref(),
                platform,
                &marker,
                &log.command_title(),
            )
            .map(Some);
        }
        crate::infra::run_lock::CommandLocks::acquire(
            root,
            &home,
            runtime.env.as_ref(),
            platform,
            &log.command_title(),
        )
        .map(Some)
    }

    pub(super) fn from_startup(
        runtime: &RuntimePolicy<'_>,
        log: &Arc<Logger>,
        token: &crate::engine::CancellationToken,
        startup: CommandStartup,
    ) -> Result<Self> {
        Self::new_with_profile_prompt(runtime, log, token, startup, profiles::prompt_interactive)
    }

    fn new_with_profile_prompt(
        runtime: &RuntimePolicy<'_>,
        log: &Arc<Logger>,
        token: &crate::engine::CancellationToken,
        startup: CommandStartup,
        prompt: impl FnOnce() -> Result<String>,
    ) -> Result<Self> {
        let platform = Platform::detect();
        let CommandStartup {
            root,
            overlay,
            run_lock,
        } = startup;
        let overlay_path = overlay.as_ref().map(|(path, _)| path.as_path());
        let (profile, source) =
            resolve_profile(runtime, &root, platform, overlay_path, log, prompt)?;
        let config = load_config(&root, &profile, platform, overlay_path, log)?;

        // UAC forwards the parent's resolved overlay as --overlay even when it
        // came from the environment. Only the original invocation may save it.
        let overlay = overlay.map(|selection| {
            if runtime.execution.elevated_child {
                selection.0
            } else {
                save_explicit_overlay(&root, selection)
            }
        });
        #[expect(clippy::print_stderr, reason = "intentional user-facing warning")]
        if !runtime.execution.elevated_child
            && source == profiles::ProfileSource::Prompt
            && let Err(e) = profiles::persist(&root, &profile.name)
        {
            eprintln!("warning: could not persist profile to git config: {e}");
        }
        let store = ConfigStore::from_config(config);
        let ctx = Self::context(runtime, log, token, root, overlay, platform)?;
        Ok(Self {
            run_lock,
            ctx,
            log: Arc::clone(log),
            store,
        })
    }

    pub(super) fn run_repository(
        runtime: &RuntimePolicy<'_>,
        log: &Arc<Logger>,
        token: &crate::engine::CancellationToken,
        task: &dyn Task,
        startup: CommandStartup,
    ) -> Result<()> {
        let platform = Platform::detect();
        let CommandStartup {
            root,
            overlay,
            run_lock: _run_lock,
        } = startup;
        // Repository-only startup does not load aggregate configuration.
        let overlay = overlay.map(|selection| save_explicit_overlay(&root, selection));
        let preview = if runtime.execution.dry_run {
            " · dry run"
        } else {
            ""
        };
        emit_startup_context(
            log.as_ref(),
            &format!(
                "{}{preview} · {}",
                log.command_title(),
                platform.description()
            ),
            runtime.repository_child,
        );
        let ctx = Self::context(runtime, log, token, root, overlay, platform)?;
        RunCoordinator::new(&ctx, log).execute(ExecutionPlan::single([task]))
    }

    fn context(
        runtime: &RuntimePolicy<'_>,
        log: &Arc<Logger>,
        token: &crate::engine::CancellationToken,
        root: PathBuf,
        overlay: Option<PathBuf>,
        platform: Platform,
    ) -> Result<Context> {
        let executor: Arc<dyn crate::infra::exec::Executor> =
            Arc::new(crate::infra::exec::ProcessExecutor::managed(token.clone()));
        let log_output: Arc<dyn Log> = Arc::<Logger>::clone(log);
        Ok(Context::new_with_policy(
            root,
            overlay,
            platform,
            log_output,
            executor,
            Arc::clone(&runtime.env),
            runtime.execution,
        )?
        .with_cancellation(token.clone()))
    }

    /// Build update tasks with repository restart state.
    #[must_use]
    pub(crate) fn update_tasks_for_run(
        &self,
        repository_update: &crate::domains::repository::update::RepositoryUpdateSignal,
        apm_mode: crate::domains::ai::apm::ApmPackageMode,
    ) -> Vec<Box<dyn Task>> {
        crate::app::catalog::update_tasks_for_run(&self.store, repository_update, apm_mode)
    }

    /// Build the full set of remove tasks, wired to the shared config store.
    #[must_use]
    pub(crate) fn remove_tasks(&self, restore_missing: bool) -> Vec<Box<dyn Task>> {
        crate::app::catalog::all_remove_tasks(&self.store, restore_missing)
    }

    /// A handle to the aggregate configuration for app-owned validation tasks.
    #[must_use]
    pub(crate) fn config_handle(&self) -> ConfigHandle<Config> {
        self.store.aggregate.clone()
    }

    pub(super) fn warn_missing_managed_sources(&self) {
        let resources =
            crate::domains::files::symlinks::build_resources(&self.ctx, self.store.symlinks.get());
        for resource in resources {
            match resource.has_missing_managed_source() {
                Ok(true) => self.log.warn(format!(
                    "managed link {} still points to a missing source {}; use the checkout's pre-update revision to recover it",
                    resource.target.display(), resource.source.display(),
                )),
                Ok(false) => {}
                Err(error) => self.log.debug(format!(
                    "could not check {} for a retired source: {error:#}", resource.target.display(),
                )),
            }
        }
    }

    /// Create dynamic overlay script tasks from the current configuration.
    #[must_use]
    pub(crate) fn overlay_script_tasks(&self) -> Vec<Box<dyn Task>> {
        self.ctx.overlay().map_or_else(Vec::new, |root| {
            let scripts = self.store.scripts.get();
            crate::domains::overlay::scripts::overlay_script_tasks(scripts, root)
        })
    }

    /// Create removal tasks for active overlay scripts.
    #[must_use]
    pub(crate) fn overlay_script_removal_tasks(&self) -> Vec<Box<dyn Task>> {
        self.ctx.overlay().map_or_else(Vec::new, |root| {
            let scripts = self.store.scripts.get();
            crate::domains::overlay::scripts::overlay_script_removal_tasks(scripts, root)
        })
    }

    /// Execute the given tasks to completion using the stored context.
    ///
    /// # Errors
    ///
    /// Returns an error if one or more tasks fail.
    pub(crate) fn run<'a>(&self, tasks: impl IntoIterator<Item = &'a dyn Task>) -> Result<()> {
        let config = self.store.aggregate.get();
        RunCoordinator::new(&self.ctx, &self.log)
            .with_profile(config.profile.name.as_str())
            .execute(ExecutionPlan::single(tasks))
    }

    pub(super) const fn run_lock(&self) -> Option<&crate::infra::run_lock::CommandLocks> {
        self.run_lock.as_ref()
    }

    /// Execute tasks and restart after a dependency boundary when requested.
    ///
    /// # Errors
    ///
    /// Returns an error if graph validation fails or one or more tasks fail.
    pub(super) fn run_with_restart<'a>(
        &'a self,
        tasks: impl IntoIterator<Item = &'a dyn Task>,
        boundary: TaskId,
        checkout: CheckoutSnapshot,
        decide: impl FnOnce() -> BoundaryAction + 'a,
        action: impl FnOnce() + 'a,
    ) -> Result<()> {
        checkout.ensure_current()?;
        let decide = move || {
            let decision = decide();
            if matches!(decision, BoundaryAction::Continue) {
                checkout.ensure_current()?;
            }
            Ok(decision)
        };
        let config = self.store.aggregate.get();
        RunCoordinator::new(&self.ctx, &self.log)
            .with_profile(config.profile.name.as_str())
            .execute(ExecutionPlan::with_restart(tasks, boundary, decide, action))
    }
}

/// Resolve the dotfiles root directory from CLI arguments or auto-detection.
///
/// # Errors
///
/// Returns an error if the root directory cannot be determined or doesn't exist.
pub(super) fn resolve_root(runtime: &RuntimePolicy<'_>) -> Result<PathBuf> {
    let cwd = std::env::current_dir().ok();
    resolve_root_from_dir(
        runtime.global.root.as_deref(),
        cwd.as_deref(),
        runtime.env.as_ref(),
    )
}

/// Resolve a repository root for standalone discovery commands.
pub(crate) fn resolve_root_path(explicit_root: Option<&Path>) -> Result<PathBuf> {
    let cwd = std::env::current_dir().ok();
    resolve_root_from_dir(explicit_root, cwd.as_deref(), &crate::infra::env::SystemEnv)
}

fn resolve_root_from_dir(
    explicit_root: Option<&Path>,
    cwd: Option<&Path>,
    env: &dyn crate::infra::env::Env,
) -> Result<PathBuf> {
    if let Some(root) = explicit_root {
        return crate::infra::fs::canonicalize(root);
    }

    if let Some(root) = env.var_os("DOTFILES_ROOT") {
        return crate::infra::fs::canonicalize(Path::new(&root));
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(root) = root_from_executable(&exe)?
    {
        return Ok(root);
    }

    if let Some(cwd) = cwd
        && cwd.join("conf").exists()
        && cwd.join("symlinks").exists()
    {
        return crate::infra::fs::canonicalize(cwd);
    }

    anyhow::bail!("cannot determine dotfiles root. Use --root or set DOTFILES_ROOT env var");
}

fn root_from_executable(exe: &Path) -> Result<Option<PathBuf>> {
    let Some(parent) = exe.parent() else {
        return Ok(None);
    };
    // An installed bin/ can sit in a worktree nested inside another checkout.
    // Look for its own checkout before trying the deeper Cargo build layout.
    for candidate in [parent.join(".."), parent.join("../../..")] {
        if candidate.join("conf").try_exists()? && candidate.join("symlinks").try_exists()? {
            return crate::infra::fs::canonicalize(&candidate).map(Some);
        }
    }
    Ok(None)
}

#[expect(clippy::print_stderr, reason = "intentional user-facing warning")]
fn save_explicit_overlay(root: &Path, (path, source): (PathBuf, OverlaySource)) -> PathBuf {
    if source == OverlaySource::Argument
        && let Err(e) = overlay_resolution::persist(root, &path)
    {
        eprintln!("warning: could not persist overlay path to git config: {e}");
    }
    path
}

fn resolve_profile(
    runtime: &RuntimePolicy<'_>,
    root: &Path,
    platform: Platform,
    overlay: Option<&Path>,
    log: &Logger,
    prompt: impl FnOnce() -> Result<String>,
) -> Result<(profiles::Profile, profiles::ProfileSource)> {
    // Run-log only: the startup header must be the first console line.
    log.run_event(LogEvent::Stage, "resolving profile");
    let restart_profile = if runtime.repository_child && !runtime.execution.elevated_child {
        Some(
            runtime
                .env
                .var(super::reexec::REPOSITORY_PROFILE_VAR)
                .ok_or_else(|| {
                    anyhow::anyhow!("repository restart is missing accepted profile; retry update")
                })?,
        )
    } else {
        None
    };
    let (profile, source) = profiles::resolve_from_args(
        restart_profile
            .as_deref()
            .or(runtime.global.profile.as_deref()),
        root,
        platform,
        runtime.env.as_ref(),
        runtime.execution.non_interactive,
        prompt,
    )?;
    let context = startup_context_line(
        &log.command_title(),
        &profile.name,
        platform,
        runtime.execution.dry_run,
        overlay,
        &crate::engine::context::home_directory(runtime.env.as_ref(), platform)?,
    );
    log.record(crate::infra::logging::records::Record::RunContext {
        profile: profile.name.clone(),
        platform: platform.description().into(),
        dry_run: runtime.execution.dry_run,
    });
    emit_startup_context(log, &context, runtime.repository_child);
    Ok((profile, source))
}

/// Emit startup context once across a repository-update restart.
pub(super) fn emit_startup_context(
    log: &dyn crate::infra::logging::Output,
    context: &str,
    repository_child: bool,
) {
    if repository_child {
        log.run_event(LogEvent::Startup, context);
    } else {
        log.startup(context);
    }
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
    overlay: Option<&Path>,
    home: &Path,
) -> String {
    let mut platform_label = platform.description().to_string();
    if platform.is_wsl() {
        platform_label.push_str(" \u{00b7} WSL");
    }
    let dry_run = if dry_run { " \u{00b7} dry run" } else { "" };
    let overlay = overlay.map_or_else(String::new, |path| {
        let display = path.strip_prefix(home).map_or_else(
            |_| path.display().to_string(),
            |relative| {
                if relative.as_os_str().is_empty() {
                    "~".into()
                } else {
                    format!("~/{}", relative.to_string_lossy().replace('\\', "/"))
                }
            },
        );
        format!(" \u{00b7} overlay {display}")
    });
    format!(
        "{command_title}{dry_run} \u{00b7} {profile_name} profile \u{00b7} {platform_label}{overlay}"
    )
}

fn load_config(
    root: &Path,
    profile: &profiles::Profile,
    platform: Platform,
    overlay: Option<&Path>,
    log: &Logger,
) -> Result<Config> {
    tracing::debug!("loading configuration");
    let config = Config::load(root, profile, platform, overlay)?;
    emit_config_summary(log, &config);

    let warnings = config.validate(platform);
    crate::app::validation::display_diagnostics(&warnings, log);

    Ok(config)
}

/// Emit the verbose-only configuration header and its non-empty section counts.
pub(super) fn emit_config_summary(log: &dyn crate::infra::logging::Output, config: &Config) {
    log.context("Loaded configuration");
    for section in config
        .section_counts()
        .iter()
        .filter(|section| section.count > 0)
    {
        log.context(format!("  {} {}", section.count, section.label()));
    }
}

#[cfg(test)]
#[path = "tests/runner.rs"]
mod root_tests;
