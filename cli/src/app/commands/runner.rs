//! Command startup composition and task-set construction.

use std::sync::Arc;

use anyhow::Result;

use crate::app::config::Config;
use crate::app::config::profiles;
use crate::app::config::store::ConfigStore;
use crate::engine::{Context, Task, TaskId};
use crate::infra::ConfigHandle;
use crate::infra::logging::{Log, LogEvent, Logger};
use crate::infra::platform::Platform;

use super::RuntimePolicy;
use super::execution::{ExecutionPlan, RunCoordinator};
use crate::infra::logging::Output as _;
use crate::infra::logging::OutputExt as _;
/// Shared orchestration helper that combines setup and task execution.
#[derive(Debug)]
pub struct CommandRunner {
    _run_lock: Option<crate::infra::run_lock::RunLock>,
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
    pub fn new(
        runtime: &RuntimePolicy<'_>,
        log: &Arc<Logger>,
        token: &crate::engine::CancellationToken,
    ) -> Result<Self> {
        let run_lock = Self::acquire_run_lock(runtime, log)?;
        Self::new_with_lock(runtime, log, token, run_lock)
    }

    pub(crate) fn acquire_run_lock(
        runtime: &RuntimePolicy<'_>,
        log: &Arc<Logger>,
    ) -> Result<Option<crate::infra::run_lock::RunLock>> {
        if runtime.execution.elevated_child || runtime.reexec_guarded {
            return Ok(None);
        }

        let platform = Platform::detect();
        let root = resolve_root(runtime)?;
        crate::infra::run_lock::RunLock::acquire(
            &root,
            runtime.env.as_ref(),
            platform,
            &log.command_title(),
        )
        .map(Some)
    }

    pub(crate) fn new_with_lock(
        runtime: &RuntimePolicy<'_>,
        log: &Arc<Logger>,
        token: &crate::engine::CancellationToken,
        run_lock: Option<crate::infra::run_lock::RunLock>,
    ) -> Result<Self> {
        let platform = Platform::detect();
        let root = resolve_root(runtime)?;
        let env = &runtime.env;
        let overlay = crate::domains::overlay::resolution::resolve_from_args(
            runtime.global.overlay.as_deref(),
            &root,
            env.as_ref(),
        )?;
        let profile = resolve_profile(runtime, &root, platform, overlay.as_deref(), log)?;
        let config = load_config(&root, &profile, platform, overlay.as_deref(), log)?;
        let store = ConfigStore::from_config(config);

        let executor: Arc<dyn crate::infra::exec::Executor> =
            Arc::new(crate::infra::exec::ProcessExecutor::managed(token.clone()));
        let log_output: Arc<dyn Log> = Arc::<Logger>::clone(log);
        let ctx = Context::new_with_policy(
            root,
            overlay,
            platform,
            log_output,
            executor,
            Arc::clone(env),
            runtime.execution,
        )?
        .with_cancellation(token.clone());
        Ok(Self {
            _run_lock: run_lock,
            ctx,
            log: Arc::clone(log),
            store,
        })
    }

    /// Build install tasks with repository restart state.
    #[must_use]
    pub(crate) fn install_tasks_for_run(
        &self,
        repository_update: &crate::domains::repository::update::RepositoryUpdateSignal,
    ) -> Vec<Box<dyn Task>> {
        crate::app::catalog::install_tasks_for_run(&self.store, repository_update)
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
        self.ctx.overlay().map_or_else(Vec::new, |root| {
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

    /// Execute tasks and restart after a dependency boundary when requested.
    ///
    /// # Errors
    ///
    /// Returns an error if graph validation fails or one or more tasks fail.
    pub fn run_with_restart<'a>(
        &'a self,
        tasks: impl IntoIterator<Item = &'a dyn Task>,
        boundary: TaskId,
        requested: impl FnOnce() -> bool + 'a,
        action: impl FnOnce() + 'a,
    ) -> Result<()> {
        RunCoordinator::new(&self.ctx, &self.log).execute(ExecutionPlan::with_restart(
            tasks, boundary, requested, action,
        ))
    }
}

/// Resolve the dotfiles root directory from CLI arguments or auto-detection.
///
/// # Errors
///
/// Returns an error if the root directory cannot be determined or doesn't exist.
pub(super) fn resolve_root(runtime: &RuntimePolicy<'_>) -> Result<std::path::PathBuf> {
    let cwd = std::env::current_dir().ok();
    resolve_root_from_dir(
        runtime.global.root.as_deref(),
        cwd.as_deref(),
        runtime.env.as_ref(),
    )
}

/// Resolve a repository root for standalone discovery commands.
pub(crate) fn resolve_root_path(
    explicit_root: Option<&std::path::Path>,
) -> Result<std::path::PathBuf> {
    let cwd = std::env::current_dir().ok();
    resolve_root_from_dir(explicit_root, cwd.as_deref(), &crate::infra::env::SystemEnv)
}

fn resolve_root_from_dir(
    explicit_root: Option<&std::path::Path>,
    cwd: Option<&std::path::Path>,
    env: &dyn crate::infra::env::Env,
) -> Result<std::path::PathBuf> {
    if let Some(root) = explicit_root {
        return crate::infra::fs::canonicalize(root);
    }

    if let Some(root) = env.var("DOTFILES_ROOT") {
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
    runtime: &RuntimePolicy<'_>,
    root: &std::path::Path,
    platform: Platform,
    overlay: Option<&std::path::Path>,
    log: &Logger,
) -> Result<profiles::Profile> {
    // Run-log only: the startup header must be the first console line.
    log.run_event(LogEvent::Stage, "resolving profile");
    let profile = profiles::resolve_from_args(
        runtime.global.profile.as_deref(),
        root,
        platform,
        runtime.env.as_ref(),
        runtime.execution.non_interactive,
    )?;
    let context = startup_context_line(
        &log.command_title(),
        &profile.name,
        platform,
        runtime.execution.dry_run,
        overlay,
    );
    if let Some(run) = crate::infra::logging::Output::run_log(log) {
        run.record(crate::infra::logging::records::Record::RunContext {
            profile: profile.name.clone(),
            platform: platform.description().into(),
            dry_run: runtime.execution.dry_run,
        });
    }
    emit_startup_context(log, &context, runtime.repository_child);
    Ok(profile)
}

/// Emit startup context once across a repository-update restart.
pub(super) fn emit_startup_context(
    log: &dyn crate::infra::logging::Output,
    context: &str,
    repository_child: bool,
) {
    if repository_child {
        log.run_event(LogEvent::Info, context);
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
mod root_tests {
    use super::*;
    use crate::infra::env::MapEnv;

    #[test]
    fn explicit_root_is_canonicalized_before_environment_selection() {
        let temp_dir = tempfile::tempdir().unwrap();
        let env = MapEnv::new().with("DOTFILES_ROOT", "/environment-root");
        for root in [temp_dir.path().to_path_buf(), temp_dir.path().join(".")] {
            let result = resolve_root_from_dir(Some(&root), None, &env).unwrap();
            assert_eq!(
                result,
                crate::infra::fs::canonicalize(temp_dir.path()).unwrap()
            );
            assert_eq!(
                resolve_root_path(Some(&root)).unwrap(),
                result,
                "standalone discovery must resolve the same explicit root"
            );
        }
    }

    #[test]
    fn only_parent_commands_acquire_the_repository_lock() {
        let root = tempfile::tempdir().unwrap();
        git2::Repository::init(root.path()).unwrap();
        let (log, _logs, _guard) = crate::infra::logging::isolated_logger();
        let log = Arc::new(log);
        for elevated in [false, true] {
            for guarded in [false, true] {
                let mut global =
                    super::super::tests::global(if elevated { &["--elevated-child"] } else { &[] });
                global.root = Some(root.path().to_path_buf());
                let env = if guarded {
                    MapEnv::new().with(super::super::reexec::REEXEC_GUARD_VAR, "")
                } else {
                    MapEnv::new()
                };
                let runtime = RuntimePolicy::new(&global, false, env.into_handle(), true, true);
                let lock = CommandRunner::acquire_run_lock(&runtime, &log).unwrap();
                assert_eq!(
                    lock.is_some(),
                    !elevated && !guarded,
                    "elevated={elevated}, guarded={guarded}"
                );
            }
        }
    }

    #[test]
    fn overlay_tasks_use_the_context_overlay_and_startup_script_snapshot() {
        for overlay in [None, Some(std::path::PathBuf::from("/fixture-overlay"))] {
            let mut config = crate::test_helpers::empty_config("/fixture-repo".into());
            config.overlay = overlay.clone();
            config
                .scripts
                .push(crate::domains::overlay::config::scripts::ScriptEntry {
                    name: "fixture".into(),
                    path: "scripts/fixture.sh".into(),
                    description: None,
                });
            let (ctx, log) = crate::test_helpers::make_static_context(config.clone());
            let runner = CommandRunner {
                _run_lock: None,
                ctx,
                log,
                store: ConfigStore::from_config(config),
            };
            let tasks = runner.overlay_script_tasks();
            let selectors: Vec<_> = tasks.iter().map(|task| task.selector()).collect();
            assert_eq!(
                selectors,
                if overlay.is_some() {
                    vec!["script-fixture"]
                } else {
                    vec![]
                }
            );
        }
    }

    #[test]
    fn environment_root_is_used_without_canonicalizing() {
        let env = MapEnv::new().with("DOTFILES_ROOT", "relative-root");
        assert_eq!(
            resolve_root_from_dir(None, None, &env).unwrap(),
            std::path::Path::new("relative-root")
        );
    }

    #[test]
    fn resolve_root_errors_when_not_in_repo() {
        let root = tempfile::tempdir().unwrap();
        let error = resolve_root_from_dir(None, Some(root.path()), &MapEnv::new()).unwrap_err();
        assert!(error.to_string().contains("cannot determine dotfiles root"));
    }

    #[test]
    fn profile_precedence_and_prompt_policy_use_the_same_runtime_environment() {
        let root = tempfile::tempdir().unwrap();
        let conf = root.path().join("conf");
        std::fs::create_dir(&conf).unwrap();
        std::fs::write(conf.join("profiles.toml"), "[base]\n[desktop]\n").unwrap();
        let repo = git2::Repository::init(root.path()).unwrap();
        repo.config()
            .unwrap()
            .set_str("dotfiles.profile", "base")
            .unwrap();
        let log = Logger::new("test");

        for (flags, env_profile, expected) in [
            (vec![], None, Some("base")),
            (vec![], Some("desktop"), Some("desktop")),
            (vec!["--profile", "base"], Some("desktop"), Some("base")),
            (vec!["--profile", "missing"], Some("desktop"), None),
            (vec![], Some("missing"), None),
        ] {
            let global = super::super::tests::global(&flags);
            let env = env_profile.map_or_else(MapEnv::new, |name| {
                MapEnv::new().with("DOTFILES_PROFILE", name)
            });
            let runtime = RuntimePolicy::new(&global, false, env.into_handle(), false, true);
            let profile = resolve_profile(&runtime, root.path(), Platform::detect(), None, &log);
            assert_eq!(
                profile.ok().map(|profile| profile.name).as_deref(),
                expected,
                "{flags:?} {env_profile:?}"
            );
        }

        repo.config().unwrap().remove("dotfiles.profile").unwrap();
        let global = super::super::tests::global(&[]);
        let runtime = RuntimePolicy::new(&global, false, MapEnv::new().into_handle(), false, true);
        let error =
            resolve_profile(&runtime, root.path(), Platform::detect(), None, &log).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("profile selection is required in non-interactive mode")
        );
    }
}
