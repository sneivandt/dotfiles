//! Top-level command handlers for install, uninstall, test, and version.
pub mod install;
pub mod logs;
pub mod test;
pub mod uninstall;
pub mod update;
pub mod version;

use std::sync::Arc;

use anyhow::Result;

use crate::cli::GlobalOpts;
use crate::config::Config;
use crate::config::profiles;
use crate::logging::{Log, Logger, Output};
use crate::platform::Platform;
use crate::tasks::TaskPhase;
use crate::tasks::{self, Context, Task};

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
        && tasks::core::self_update::pre_update(&root, &**log, global.dry_run)?
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

    std::process::Command::new(crate::elevation::preferred_powershell())
        .args([
            "-NoProfile",
            "-EncodedCommand",
            &crate::elevation::powershell_encode_command(&helper_script),
        ])
        .spawn()
        .context("spawning restart helper")?;

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
        exe = crate::elevation::powershell_single_quote(&exe.display().to_string()),
        pending = crate::elevation::powershell_single_quote(&pending.display().to_string()),
        pending_version =
            crate::elevation::powershell_single_quote(&pending_version.display().to_string()),
        cache = crate::elevation::powershell_single_quote(&cache.display().to_string()),
        args = crate::elevation::powershell_arg_list(args),
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
        let overlay = resolve_overlay(global, &root, &**log);
        let profile = resolve_profile(global, &root, platform, &**log)?;
        let config = load_config(&root, &profile, platform, overlay.as_deref(), &**log)?;

        let executor: Arc<dyn crate::exec::Executor> = Arc::new(crate::exec::SystemExecutor);
        let ctx = Context::new(
            Arc::new(std::sync::RwLock::new(Arc::new(config))),
            platform,
            Arc::clone(log) as Arc<dyn Log>,
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
        })
    }

    /// Return a runner whose context advances locked dependency versions.
    ///
    /// Used by the `update` command so that version-advancing tasks (currently
    /// the APM dependency refresh) run; `install` leaves this `false`.
    #[must_use]
    pub fn with_advance_versions(mut self, advance_versions: bool) -> Self {
        self.ctx = self.ctx.with_advance_versions(advance_versions);
        self
    }

    /// Create dynamic overlay script tasks from the loaded configuration.
    ///
    /// Returns one [`OverlayScriptTask`](crate::tasks::overlay::OverlayScriptTask)
    /// per script entry in the overlay config.  Returns an empty list when no
    /// overlay is configured.
    #[must_use]
    pub fn overlay_script_tasks(&self) -> Vec<Box<dyn Task>> {
        let config = self.ctx.config_read();
        config.overlay.as_ref().map_or_else(Vec::new, |root| {
            tasks::overlay::overlay_script_tasks(&config.scripts, root)
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
    log: &dyn Output,
) -> Result<profiles::Profile> {
    log.stage("Resolving profile");
    let profile = profiles::resolve_from_args(global.profile.as_deref(), root, platform)?;
    let mut platform_label = platform.description().to_string();
    if platform.is_wsl() {
        platform_label.push_str(" \u{00b7} WSL");
    }
    log.always(&format!(
        "\x1b[2mprofile\x1b[0m  {} \x1b[2m\u{00b7} {platform_label}\x1b[0m",
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
    let overlay = crate::config::overlay::resolve_from_args(global.overlay.as_deref(), root);
    if let Some(ref path) = overlay {
        log.always(&format!("\x1b[2moverlay\x1b[0m  {}", path.display()));
    }
    overlay
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
    log: &dyn Output,
) -> Result<Config> {
    log.stage("Loading configuration");
    let config = Config::load(root, profile, platform, overlay)?;

    let debug_count = |count: usize, label: &str| log.debug(&format!("{count} {label}"));

    let counts = [
        (config.packages.len(), "packages"),
        (config.symlinks.len(), "symlinks"),
        (config.registry.len(), "registry entries"),
        (config.units.len(), "systemd units"),
        (config.chmod.len(), "chmod entries"),
        (config.vscode_extensions.len(), "vscode extensions"),
        (config.manifest.excluded_files.len(), "manifest exclusions"),
        (config.scripts.len(), "overlay scripts"),
    ];

    for (count, label) in &counts {
        debug_count(*count, label);
    }

    let warnings = config.validate(platform);
    crate::config::helpers::validation::display_validation_warnings(&warnings, log);

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

    if !ctx.executor.which("sudo") {
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

    log.always(&format!("sudo is required for: {}", task_names.join(", ")));
    // Flush stdout so the phase header is visible before the password prompt.
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
            log.error("sudo credential priming failed");
            false
        }
        Err(e) => {
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
/// Update).
///
/// Phases run strictly in order, each completing before the next begins; an
/// empty phase (e.g. Update under `install`) is skipped with no header.  Within
/// a phase, when parallel execution is enabled, tasks are dispatched through the
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

        log.always("");
        log.phase(phase.label());

        // Before parallel dispatch, prime the sudo credential cache if any
        // task in this phase will need root privileges.  This avoids an
        // interactive password prompt appearing mid-way through interleaved
        // parallel output.  If priming fails, record sudo-dependent tasks as
        // failed and exclude them from this phase's dispatch.
        let sudo_task_names: Vec<&str> = if ctx.parallel && !ctx.dry_run && phase_tasks.len() > 1 {
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
                    log.record_task_outcome(
                        task.name(),
                        task.domain(),
                        crate::logging::TaskStatus::Skipped,
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
        // keeps the Update phase's `update:` line below its check mark, the
        // same way the Provision phase renders the `install:` line.
        if ctx.parallel && !phase_tasks.is_empty() {
            let graph = match crate::engine::graph::ResolvedTaskGraph::resolve(&phase_tasks) {
                Ok(graph) => graph,
                Err(err) => {
                    let message = format!("{err} detected in {phase} phase task graph");
                    log.error(&message);
                    anyhow::bail!(message);
                }
            };
            crate::engine::scheduler::run_tasks_parallel(&phase_tasks, &graph, ctx, log);
        } else {
            for task in &phase_tasks {
                if ctx.is_cancelled() {
                    log.warn("cancelled - stopping before next task");
                    break;
                }
                tasks::execute(*task, ctx);
            }
        }
    }

    log.print_summary();

    let count = log.failure_count();
    if count > 0 {
        anyhow::bail!("{count} task(s) failed");
    }
    Ok(())
}
