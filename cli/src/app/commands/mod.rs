//! Top-level command handlers for install, uninstall, test, and version.
pub mod install;
pub mod log;
pub mod test;
pub mod uninstall;
pub mod update;
pub mod version;

use std::sync::Arc;

use anyhow::Result;

use crate::app::cli::GlobalOpts;
use crate::app::config::Config;
use crate::app::config::profiles;
use crate::app::config::store::ConfigStore;
use crate::engine::TaskPhase;
use crate::engine::{Context, Task};
use crate::runtime::ConfigHandle;
use crate::runtime::logging::{Log, Logger, Output};
use crate::runtime::platform::Platform;

/// Environment variable set before re-exec to prevent infinite self-update loops.
const REEXEC_GUARD_VAR: &str = "DOTFILES_REEXEC_GUARD";

/// Exit code used on Windows after staging a self-update so the restart helper
/// knows the binary exited intentionally.
#[cfg(windows)]
const WINDOWS_RESTART_EXIT_CODE: i32 = 75;

/// Replace the current process with a fresh invocation of the same binary.
///
/// Called after a self-update has replaced the binary on disk so that the
/// new version runs all tasks with updated code.  Sets [`REEXEC_GUARD_VAR`]
/// so the new process skips the self-update step.
#[allow(unused_variables, reason = "root is platform-specific re-exec context")]
pub(crate) fn re_exec(root: &std::path::Path, log: &dyn Output) -> ! {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let args: Vec<String> = std::env::args().skip(1).collect();
        let exe = re_exec_path(root);
        let err = std::process::Command::new(&exe)
            .args(&args)
            .env(REEXEC_GUARD_VAR, "1")
            .exec();
        log.error(&format!("failed to re-exec: {err}"));
        std::process::exit(1);
    }

    #[cfg(windows)]
    {
        if let Err(err) = spawn_windows_restart_helper() {
            log.error(&format!("failed to schedule Windows restart: {err}"));
            std::process::exit(1);
        }

        std::process::exit(WINDOWS_RESTART_EXIT_CODE);
    }

    #[cfg(all(not(unix), not(windows)))]
    {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let exe = re_exec_path(root).unwrap_or_else(|err| {
            log.error(&format!("cannot determine executable path: {err}"));
            std::process::exit(1);
        });
        match std::process::Command::new(&exe)
            .args(&args)
            .env(REEXEC_GUARD_VAR, "1")
            .status()
        {
            Ok(s) => {
                if s.code().is_none() {
                    log.warn("child process terminated by signal");
                }
                std::process::exit(s.code().unwrap_or(1))
            }
            Err(e) => {
                log.error(&format!("failed to re-exec: {e}"));
                std::process::exit(1);
            }
        }
    }
}

/// Run the shared self-update preflight and re-exec if the binary changed.
///
/// # Errors
///
/// Returns an error if the repository root cannot be resolved or the pre-update
/// check fails.
pub(crate) fn prepare_self_update(global: &GlobalOpts, log: &Arc<Logger>) -> Result<()> {
    let root = install::resolve_root(global)?;
    if std::env::var_os(REEXEC_GUARD_VAR).is_none()
        && crate::domains::dotfiles::self_update::pre_update(&root, &**log, global.dry_run)?
    {
        re_exec(&root, &**log);
    }
    Ok(())
}

#[cfg(unix)]
fn re_exec_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join("bin").join("dotfiles")
}

#[cfg(not(unix))]
#[cfg_attr(windows, allow(dead_code, reason = "used conditionally via cfg"))]
/// Resolve the executable path used for re-exec on non-Unix platforms.
///
/// The `root` parameter is retained so the Unix and non-Unix variants share
/// the same call signature even though non-Unix re-exec always restarts the
/// current executable.
fn re_exec_path(_root: &std::path::Path) -> Result<std::path::PathBuf> {
    use anyhow::Context as _;

    std::env::current_exe().context("determining current executable path for re-exec")
}

#[cfg(windows)]
fn spawn_windows_restart_helper() -> Result<()> {
    use anyhow::Context as _;

    let exe = std::env::current_exe().context("determining current executable path")?;
    let exe_dir = exe
        .parent()
        .context("determining executable directory for staged update")?;

    let pending = exe_dir.join(".dotfiles-update.pending");
    let pending_version = exe_dir.join(".dotfiles-update.version");
    let cache = exe_dir.join(".dotfiles-version-cache");
    let args: Vec<String> = std::env::args().skip(1).collect();

    let helper_script =
        build_windows_restart_helper_script(&exe, &pending, &pending_version, &cache, &args);

    let mut command = std::process::Command::new(crate::runtime::elevation::preferred_powershell());
    crate::runtime::exec::windows::PowerShellCommand::new(&helper_script).configure(&mut command);
    command.spawn().context("spawning restart helper")?;

    Ok(())
}

#[cfg(windows)]
fn build_windows_restart_helper_script(
    exe: &std::path::Path,
    pending: &std::path::Path,
    pending_version: &std::path::Path,
    cache: &std::path::Path,
    args: &[String],
) -> String {
    format!(
        "$exe = {exe}; \
                 $pending = {pending}; \
                 $pendingVersion = {pending_version}; \
                 $cache = {cache}; \
                 $args = {args}; \
                 for ($attempt = 0; $attempt -lt 50; $attempt++) {{ \
                     Start-Sleep -Milliseconds 200; \
                     try {{ \
                         if (Test-Path $pending) {{ \
                             $backup = $exe + '.bak'; \
                             if (Test-Path $exe) {{ Move-Item -Path $exe -Destination $backup -Force }}; \
                             try {{ \
                                 Move-Item -Path $pending -Destination $exe -Force \
                             }} catch {{ \
                                 if (Test-Path $backup) {{ Move-Item -Path $backup -Destination $exe -Force }}; \
                                 throw \
                             }}; \
                             if (Test-Path $backup) {{ Remove-Item $backup -Force }} \
                         }}; \
                         if (Test-Path $pendingVersion) {{ \
                             $version = (Get-Content $pendingVersion -ErrorAction Stop | Select-Object -First 1).Trim(); \
                             if (-not [string]::IsNullOrWhiteSpace($version)) {{ \
                                 $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds(); \
                                 Set-Content -Path $cache -Value @($version, $timestamp) -Encoding utf8 \
                             }}; \
                             Remove-Item $pendingVersion -Force \
                         }}; \
                         $env:{guard} = '1'; \
                         & $exe @args; \
                         exit $LASTEXITCODE \
                     }} catch {{ \
                         if ($attempt -eq 49) {{ throw }} \
                     }} \
                 }}; \
                 exit 1",
        exe = crate::runtime::exec::windows::powershell_single_quote(&exe.display().to_string()),
        pending =
            crate::runtime::exec::windows::powershell_single_quote(&pending.display().to_string()),
        pending_version = crate::runtime::exec::windows::powershell_single_quote(
            &pending_version.display().to_string()
        ),
        cache =
            crate::runtime::exec::windows::powershell_single_quote(&cache.display().to_string()),
        args = crate::runtime::exec::windows::powershell_arg_list(args),
        guard = REEXEC_GUARD_VAR,
    )
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

/// Shared orchestration helper that combines setup and task execution.
///
/// Handles platform detection, profile resolution, config loading,
/// `Context` construction, and task execution in a single entry point.
#[derive(Debug)]
pub struct CommandRunner {
    ctx: Context,
    log: Arc<Logger>,
    store: ConfigStore,
    overlay: Option<std::path::PathBuf>,
}

impl CommandRunner {
    /// Initialize by detecting the platform, resolving the profile, loading
    /// configuration, and building the task execution context.
    ///
    /// # Errors
    ///
    /// Returns an error if setup fails (profile resolution, configuration
    /// loading, or the HOME environment variable is not set).
    pub fn new(
        global: &GlobalOpts,
        log: &Arc<Logger>,
        token: &crate::engine::CancellationToken,
    ) -> Result<Self> {
        let platform = Platform::detect();
        let root = install::resolve_root(global)?;
        let updated = std::env::var_os(REEXEC_GUARD_VAR).is_some();
        let profile = resolve_profile(global, &root, platform, updated, &**log)?;
        let overlay = resolve_overlay(global, &root, &**log);
        if log.is_verbose() {
            log.separate_from_startup();
        }
        let config = load_config(&root, &profile, platform, overlay.as_deref(), log)?;
        let store = ConfigStore::from_config(config);

        let executor: Arc<dyn crate::runtime::exec::Executor> =
            Arc::new(crate::runtime::exec::ManagedExecutor::new(token.clone()));
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

    /// Create dynamic overlay script tasks from the startup configuration.
    ///
    /// Returns one [`OverlayScriptTask`](crate::domains::overlay::tasks::OverlayScriptTask)
    /// per script entry in the overlay config. The returned list is a startup
    /// snapshot and is not rebuilt after repository synchronization. Returns an
    /// empty list when no overlay is configured.
    #[must_use]
    pub fn overlay_script_tasks(&self) -> Vec<Box<dyn Task>> {
        self.overlay.as_ref().map_or_else(Vec::new, |root| {
            let scripts = self.store.scripts.read();
            crate::domains::overlay::tasks::overlay_script_tasks(&scripts, root)
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
/// be loaded from `conf/profiles.toml`, or interactive prompting fails.
fn resolve_profile(
    global: &GlobalOpts,
    root: &std::path::Path,
    platform: Platform,
    updated: bool,
    log: &dyn Output,
) -> Result<profiles::Profile> {
    log.stage("Resolving profile");
    let profile = profiles::resolve_from_args(global.profile.as_deref(), root, platform)?;
    let version =
        option_env!("DOTFILES_VERSION").unwrap_or(concat!("dev-", env!("CARGO_PKG_VERSION")));
    let updated_label = if updated {
        " \x1b[2m\u{00b7} refreshed\x1b[0m"
    } else {
        ""
    };
    let mut platform_label = platform.description().to_string();
    if platform.is_wsl() {
        platform_label.push_str(" \u{00b7} WSL");
    }
    log.always(&format!(
        "\x1b[2mversion\x1b[0m {version}{updated_label} \x1b[2m\u{00b7} profile\x1b[0m {} \x1b[2m\u{00b7} {platform_label}\x1b[0m",
        profile.name
    ));
    Ok(profile)
}

/// Resolve the overlay path from CLI args, `DOTFILES_OVERLAY` env var, or
/// persisted git config, logging the result.
fn resolve_overlay(
    global: &GlobalOpts,
    root: &std::path::Path,
    log: &dyn Output,
) -> Option<std::path::PathBuf> {
    let overlay = crate::domains::overlay::config::overlay::resolve_from_args(
        global.overlay.as_deref(),
        root,
    );
    log_overlay_path(overlay.as_deref(), log);
    overlay
}

fn log_overlay_path(overlay: Option<&std::path::Path>, log: &dyn Output) {
    if let Some(path) = overlay {
        log.always(&format!("\x1b[2moverlay\x1b[0m {}", path.display()));
    }
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
    platform: Platform,
    overlay: Option<&std::path::Path>,
    log: &Logger,
) -> Result<Config> {
    log.stage("Loading configuration");
    let config = Config::load(root, profile, platform, overlay)?;

    for section in config.section_counts() {
        log.debug(&format!("{} {}", section.count, section.label));
    }

    let warnings = config.validate(platform);
    if !warnings.is_empty() && !log.is_verbose() {
        log.separate_from_startup();
    }
    crate::runtime::config_support::validation::display_diagnostics(&warnings, log);

    Ok(config)
}

/// Prime the sudo credential cache so that later parallel tasks can run
/// privileged commands without an interactive prompt.
///
/// Logs which tasks require sudo (from `task_names`) before prompting, then
/// runs `sudo -v` which validates (and caches) the user's credentials.
///
/// Returns `true` if credentials were successfully cached, `false` on any
/// failure (sudo not found, user cancelled the prompt, wrong password, etc.).
#[cfg(unix)]
fn prime_sudo(ctx: &Context, log: &Arc<Logger>, task_names: &[&str]) -> bool {
    use std::process::Stdio;

    if !ctx.executor().which("sudo") {
        log.separate_from_startup();
        log.warn("sudo not found on PATH");
        return false;
    }
    log.debug("priming sudo credential cache");

    // Check whether credentials are already cached (non-interactive).
    let already_cached = std::process::Command::new("sudo")
        .args(["-n", "-v"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());

    if already_cached {
        log.debug("sudo credentials already cached");
        return true;
    }

    log.separate_from_startup();
    log.always(&format!("sudo is required for: {}", task_names.join(", ")));
    // Flush stdout so the sudo notice is visible before the password prompt.
    drop(std::io::Write::flush(&mut std::io::stdout()));
    // Connect sudo directly to /dev/tty so the password prompt and keyboard
    // input work correctly regardless of how the Rust process's stdio is
    // configured (the tracing subscriber splits stdout/stderr).
    let tty_in = std::fs::File::open("/dev/tty");
    let tty_out = std::fs::OpenOptions::new().write(true).open("/dev/tty");
    let mut cmd = std::process::Command::new("sudo");
    cmd.arg("-v");
    if let Ok(f) = tty_in {
        cmd.stdin(Stdio::from(f));
    }
    if let Ok(f) = tty_out {
        cmd.stderr(Stdio::from(f));
    }
    match cmd.status() {
        Ok(status) if status.success() => true,
        Ok(_) => {
            log.separate_from_startup();
            log.error("sudo credential priming failed");
            false
        }
        Err(e) => {
            log.separate_from_startup();
            log.error(&format!("failed to run sudo: {e:#}"));
            false
        }
    }
}

#[cfg(not(unix))]
const fn prime_sudo(_ctx: &Context, _log: &Arc<Logger>, _task_names: &[&str]) -> bool {
    true
}

/// Execute the full phased task pipeline (Bootstrap → Sync → Provision →
/// Validation → Update).
///
/// Phases run strictly in order, each completing before the next begins.
/// Within a phase, when parallel execution is enabled, tasks are dispatched through the
/// buffered scheduler and run as soon as their dependencies complete; each
/// task's console output is buffered and flushed atomically on completion so
/// the result header is shown above any per-task detail lines.
/// A status line shows which tasks are currently running.
///
/// When parallel execution is disabled, tasks execute sequentially in list
/// order with their output written directly.
///
/// # Errors
///
/// Returns an error if parallel graph validation fails or if one or more tasks
/// recorded a failure.
pub fn run_tasks_to_completion<'a>(
    tasks: impl IntoIterator<Item = &'a dyn Task>,
    ctx: &Context,
    log: &Arc<Logger>,
) -> Result<()> {
    let tasks: Vec<&dyn Task> = tasks.into_iter().collect();
    let phases = [
        TaskPhase::Bootstrap,
        TaskPhase::Sync,
        TaskPhase::Provision,
        TaskPhase::Validation,
        TaskPhase::Update,
    ];

    for phase in phases {
        if ctx.is_cancelled() {
            log.warn("cancelled - stopping before next phase");
            break;
        }

        let mut phase_tasks: Vec<&dyn Task> = tasks
            .iter()
            .copied()
            .filter(|task| task.phase() == phase)
            .collect();

        if phase_tasks.is_empty() {
            continue;
        }

        // Before parallel dispatch, prime the sudo credential cache if any
        // task in this phase will need root privileges.  This avoids an
        // interactive password prompt appearing mid-way through interleaved
        // parallel output.  If priming fails, record sudo-dependent tasks as
        // failed and exclude them from this phase's dispatch.
        let sudo_task_names: Vec<&str> =
            if ctx.parallel() && !ctx.dry_run() && phase_tasks.len() > 1 {
                phase_tasks
                    .iter()
                    .filter(|t| t.requires_elevation(ctx))
                    .map(|t| t.name())
                    .collect()
            } else {
                Vec::new()
            };
        let sudo_failed = !sudo_task_names.is_empty() && !prime_sudo(ctx, log, &sudo_task_names);

        if sudo_failed {
            // If the user interrupted the sudo prompt (Ctrl+C), the
            // cancellation token is now set.  Honour it immediately so we
            // don't dispatch a batch of non-sudo tasks the user didn't
            // ask for.
            if ctx.is_cancelled() {
                log.warn("cancelled - stopping before next phase");
                break;
            }

            let reason = "sudo credentials unavailable";
            phase_tasks.retain(|task| {
                if task.requires_elevation(ctx) {
                    let span = tracing::info_span!("task", name = task.name());
                    let _enter = span.enter();
                    log.info(&format!("skipped: {reason}"));
                    log.record_task(
                        task.name(),
                        crate::runtime::logging::TaskStatus::Skipped,
                        Some(reason),
                    );
                    false
                } else {
                    true
                }
            });
        }

        // A phase with a single task still dispatches through the buffered
        // scheduler (not the sequential fallback) so its result header is
        // replayed before any detail lines — matching multi-task phases.  This
        // keeps the Update phase's `updated:` line below its check mark, the
        // same way the Provision phase renders the `installed:` line.
        if phase_tasks.is_empty() {
            continue;
        }

        let graph = match crate::engine::graph::ResolvedTaskGraph::resolve(&phase_tasks) {
            Ok(graph) => graph,
            Err(err) => {
                let message = format!("{err} detected in {phase} phase task graph");
                log.error(&message);
                anyhow::bail!(message);
            }
        };

        if ctx.parallel() {
            crate::engine::scheduler::run_tasks_parallel(&phase_tasks, &graph, ctx, log);
        } else {
            crate::engine::scheduler::run_tasks_sequential(&phase_tasks, &graph, ctx, log);
        }
    }

    log.print_summary();

    let count = log.failure_count();
    if count > 0 {
        return Err(crate::runtime::error::TaskFailures::new(count).into());
    }
    Ok(())
}
