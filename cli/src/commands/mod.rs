//! Top-level command handlers for install, uninstall, test, and version.
pub mod install;
pub mod test;
pub mod uninstall;
pub mod version;

use std::sync::Arc;

use anyhow::Result;

use crate::cli::GlobalOpts;
use crate::config::Config;
use crate::config::profiles;
use crate::logging::{Log, Logger, Output};
use crate::phases::TaskPhase;
use crate::phases::{self, Context, Task};
use crate::platform::Platform;

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
#[allow(unused_variables, clippy::print_stderr)]
pub(crate) fn re_exec(root: &std::path::Path) -> ! {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let args: Vec<String> = std::env::args().skip(1).collect();
        let exe = re_exec_path(root);
        let err = std::process::Command::new(&exe)
            .args(&args)
            .env(REEXEC_GUARD_VAR, "1")
            .exec();
        eprintln!("\x1b[31mError: failed to re-exec: {err}\x1b[0m");
        std::process::exit(1);
    }

    #[cfg(windows)]
    {
        if let Err(err) = spawn_windows_restart_helper() {
            eprintln!("\x1b[31mError: failed to schedule Windows restart: {err}\x1b[0m");
            std::process::exit(1);
        }

        std::process::exit(WINDOWS_RESTART_EXIT_CODE);
    }

    #[cfg(all(not(unix), not(windows)))]
    {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let exe = re_exec_path(root).unwrap_or_else(|err| {
            eprintln!("\x1b[31mError: cannot determine executable path: {err}\x1b[0m");
            std::process::exit(1);
        });
        match std::process::Command::new(&exe)
            .args(&args)
            .env(REEXEC_GUARD_VAR, "1")
            .status()
        {
            Ok(s) => {
                if s.code().is_none() {
                    eprintln!("\x1b[33mWarning: child process terminated by signal\x1b[0m");
                }
                std::process::exit(s.code().unwrap_or(1))
            }
            Err(e) => {
                eprintln!("\x1b[31mError: failed to re-exec: {e}\x1b[0m");
                std::process::exit(1);
            }
        }
    }
}

#[cfg(unix)]
fn re_exec_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join("bin").join("dotfiles")
}

#[cfg(not(unix))]
#[cfg_attr(windows, allow(dead_code))]
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

    std::process::Command::new(preferred_powershell())
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
        exe = powershell_single_quote(&exe.display().to_string()),
        pending = powershell_single_quote(&pending.display().to_string()),
        pending_version = powershell_single_quote(&pending_version.display().to_string()),
        cache = powershell_single_quote(&cache.display().to_string()),
        args = powershell_array_literal(args),
        guard = REEXEC_GUARD_VAR,
    )
}

#[cfg(windows)]
fn preferred_powershell() -> &'static str {
    use std::process::Stdio;

    if std::process::Command::new("pwsh")
        .args(["-NoProfile", "-Command", "exit 0"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        "pwsh"
    } else {
        "powershell"
    }
}

#[cfg(windows)]
fn powershell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(windows)]
fn powershell_array_literal(values: &[String]) -> String {
    if values.is_empty() {
        "@()".to_string()
    } else {
        format!(
            "@({})",
            values
                .iter()
                .map(|value| powershell_single_quote(value))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

#[cfg(all(test, windows))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn powershell_array_literal_preserves_spaces_and_quotes() {
        let args = vec![
            "C:\\Temp\\Path With Space".to_string(),
            "O'Brien".to_string(),
        ];

        let literal = powershell_array_literal(&args);

        assert_eq!(literal, "@('C:\\Temp\\Path With Space', 'O''Brien')");
    }

    #[test]
    fn windows_restart_helper_script_relaunches_with_splatting_and_guard() {
        let script = build_windows_restart_helper_script(
            Path::new("C:\\Program Files\\dotfiles.exe"),
            Path::new("C:\\Program Files\\.dotfiles-update.pending"),
            Path::new("C:\\Program Files\\.dotfiles-update.version"),
            Path::new("C:\\Program Files\\.dotfiles-version-cache"),
            &["--root".to_string(), "C:\\Users\\Me\\My Repo".to_string()],
        );

        assert!(script.contains("$env:DOTFILES_REEXEC_GUARD = '1';"));
        assert!(script.contains("& $exe @args;"));
        assert!(script.contains("exit $LASTEXITCODE"));
        assert!(!script.contains("Start-Process -FilePath $exe -ArgumentList $args"));
    }

    #[test]
    fn windows_restart_helper_script_uses_safe_atomic_update() {
        let script = build_windows_restart_helper_script(
            Path::new("C:\\Program Files\\dotfiles.exe"),
            Path::new("C:\\Program Files\\.dotfiles-update.pending"),
            Path::new("C:\\Program Files\\.dotfiles-update.version"),
            Path::new("C:\\Program Files\\.dotfiles-version-cache"),
            &["--root".to_string()],
        );

        // The exe must NOT be deleted directly before the pending file is moved.
        assert!(
            !script.contains("Remove-Item $exe"),
            "script must not delete $exe before the pending file is in place"
        );

        // The backup must be created (by moving $exe) before the pending move.
        let backup_pos = script
            .find("Move-Item -Path $exe -Destination $backup")
            .expect("script must back up $exe before moving $pending");
        let move_pending_pos = script
            .find("Move-Item -Path $pending -Destination $exe")
            .expect("script must move $pending to $exe");
        assert!(
            backup_pos < move_pending_pos,
            "backup of $exe must precede the move of $pending into place"
        );

        // On failure, the backup must be restored before rethrowing.
        assert!(
            script.contains("Move-Item -Path $backup -Destination $exe -Force"),
            "script must restore $exe from backup on failure"
        );

        // On success, the backup must be cleaned up.
        assert!(
            script.contains("Remove-Item $backup -Force"),
            "script must remove the backup after a successful update"
        );
    }
}

#[cfg(test)]
#[cfg(unix)]
mod unix_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn re_exec_path_uses_installed_binary_path() {
        let root = Path::new("/repo");
        assert_eq!(re_exec_path(root), root.join("bin").join("dotfiles"));
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod task_graph_tests {
    use super::*;
    use crate::phases::{
        TaskResult, task_deps,
        test_helpers::{empty_config, make_static_context},
    };
    use anyhow::Result;
    use std::path::PathBuf;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    struct CycleTaskA {
        ran: Arc<AtomicBool>,
    }

    impl Task for CycleTaskA {
        fn name(&self) -> &'static str {
            "cycle-a"
        }

        fn phase(&self) -> TaskPhase {
            TaskPhase::Apply
        }

        task_deps![CycleTaskB];

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    struct CycleTaskB {
        ran: Arc<AtomicBool>,
    }

    impl Task for CycleTaskB {
        fn name(&self) -> &'static str {
            "cycle-b"
        }

        fn phase(&self) -> TaskPhase {
            TaskPhase::Apply
        }

        task_deps![CycleTaskA];

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    #[test]
    fn run_tasks_to_completion_bails_on_dependency_cycles() {
        let (ctx, log) = make_static_context(empty_config(PathBuf::from("/repo")));
        let ctx = ctx.with_parallel(true);
        let ran_a = Arc::new(AtomicBool::new(false));
        let ran_b = Arc::new(AtomicBool::new(false));
        let task_a = CycleTaskA {
            ran: Arc::clone(&ran_a),
        };
        let task_b = CycleTaskB {
            ran: Arc::clone(&ran_b),
        };

        let err = run_tasks_to_completion([&task_a as &dyn Task, &task_b as &dyn Task], &ctx, &log)
            .expect_err("cyclic task graphs should fail fast");

        assert!(format!("{err:#}").contains("dependency cycle detected"));
        assert!(!ran_a.load(Ordering::SeqCst));
        assert!(!ran_b.load(Ordering::SeqCst));
    }

    struct BootstrapMarkTask {
        name: &'static str,
        completed: Arc<AtomicUsize>,
    }

    impl Task for BootstrapMarkTask {
        fn name(&self) -> &'static str {
            self.name
        }

        fn phase(&self) -> TaskPhase {
            TaskPhase::Bootstrap
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.completed.fetch_add(1, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    struct ApplyAfterBootstrapTask {
        ran: Arc<AtomicBool>,
        completed_bootstrap: Arc<AtomicUsize>,
        expected_bootstrap_count: usize,
    }

    impl Task for ApplyAfterBootstrapTask {
        fn name(&self) -> &'static str {
            "apply-after-bootstrap"
        }

        fn phase(&self) -> TaskPhase {
            TaskPhase::Apply
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.store(true, Ordering::SeqCst);
            let done = self.completed_bootstrap.load(Ordering::SeqCst);
            if done != self.expected_bootstrap_count {
                return Ok(TaskResult::Failed(format!(
                    "apply started before bootstrap completed: {done}/{}",
                    self.expected_bootstrap_count
                )));
            }
            Ok(TaskResult::Ok)
        }
    }

    #[test]
    fn run_tasks_to_completion_completes_bootstrap_phase_before_apply() {
        let (ctx, log) = make_static_context(empty_config(PathBuf::from("/repo")));
        let ctx = ctx.with_parallel(true);

        let completed_bootstrap = Arc::new(AtomicUsize::new(0));
        let apply_ran = Arc::new(AtomicBool::new(false));

        let bootstrap = BootstrapMarkTask {
            name: "bootstrap-mark",
            completed: Arc::clone(&completed_bootstrap),
        };
        let apply = ApplyAfterBootstrapTask {
            ran: Arc::clone(&apply_ran),
            completed_bootstrap: Arc::clone(&completed_bootstrap),
            expected_bootstrap_count: 1,
        };

        // Intentionally pass apply first to ensure phase gating, not input
        // order, controls execution.
        run_tasks_to_completion([&apply as &dyn Task, &bootstrap as &dyn Task], &ctx, &log)
            .expect("phase barriers should run all bootstrap tasks before apply");

        assert_eq!(completed_bootstrap.load(Ordering::SeqCst), 1);
        assert!(apply_ran.load(Ordering::SeqCst));
    }
}

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
                is_ci: None,
            },
        )?
        .with_cancellation(token.clone());

        Ok(Self {
            ctx,
            log: Arc::clone(log),
        })
    }

    /// Create dynamic overlay script tasks from the loaded configuration.
    ///
    /// Returns one [`OverlayScriptTask`](crate::phases::apply::overlay_scripts::OverlayScriptTask)
    /// per script entry in the overlay config.  Returns an empty list when no
    /// overlay is configured.
    #[must_use]
    pub fn overlay_script_tasks(&self) -> Vec<Box<dyn Task>> {
        let config = self.ctx.config_read();
        config.overlay.as_ref().map_or_else(Vec::new, |root| {
            phases::apply::overlay_scripts::overlay_script_tasks(&config.scripts, root)
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
    platform: Platform,
    log: &dyn Output,
) -> Result<profiles::Profile> {
    log.stage("Resolving profile");
    let profile = profiles::resolve_from_args(global.profile.as_deref(), root, platform)?;
    log.always(&format!("  profile: {}", profile.name));
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
        log.always(&format!("  overlay: {}", path.display()));
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
        (config.copilot_plugins.len(), "copilot plugins"),
        (config.manifest.excluded_files.len(), "manifest exclusions"),
        (config.scripts.len(), "overlay scripts"),
    ];

    for (count, label) in &counts {
        debug_count(*count, label);
    }

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
    std::io::Write::flush(&mut std::io::stdout()).ok();
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

/// Execute the full three-phase task pipeline.
///
/// task is present, tasks run as soon as their dependencies complete.  Each
/// task's console output is buffered and flushed atomically on completion.
/// A status line shows which tasks are currently running.
///
/// When parallel execution is disabled (or only one task is present),
/// tasks execute sequentially in list order.
///
/// # Errors
///
/// Returns an error if the task graph contains a dependency cycle or if one or
/// more tasks recorded a failure.
pub fn run_tasks_to_completion<'a>(
    tasks: impl IntoIterator<Item = &'a dyn Task>,
    ctx: &Context,
    log: &Arc<Logger>,
) -> Result<()> {
    let tasks: Vec<&dyn Task> = tasks.into_iter().collect();
    let phases = [
        TaskPhase::Bootstrap,
        TaskPhase::Repository,
        TaskPhase::Apply,
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
        log.phase(&phase.to_string());

        // Before parallel dispatch, prime the sudo credential cache if any
        // task in this phase will need root privileges.  This avoids an
        // interactive password prompt appearing mid-way through interleaved
        // parallel output.  If priming fails, record sudo-dependent tasks as
        // failed and exclude them from this phase's dispatch.
        let sudo_task_names: Vec<&str> = if ctx.parallel && !ctx.dry_run && phase_tasks.len() > 1 {
            phase_tasks
                .iter()
                .filter(|t| t.needs_sudo(ctx))
                .map(|t| t.name())
                .collect()
        } else {
            Vec::new()
        };
        let sudo_failed = !sudo_task_names.is_empty() && !prime_sudo(ctx, log, &sudo_task_names);

        if sudo_failed {
            let reason = "sudo credentials unavailable";
            phase_tasks.retain(|task| {
                if task.needs_sudo(ctx) {
                    log.record_task(
                        task.name(),
                        task.phase(),
                        crate::logging::TaskStatus::Skipped,
                        Some(reason),
                    );
                    log.emit_task_result(
                        task.name(),
                        &crate::logging::TaskStatus::Skipped,
                        Some(reason),
                    );
                    false
                } else {
                    true
                }
            });
        }

        if ctx.parallel && phase_tasks.len() > 1 {
            if phases::has_cycle(&phase_tasks) {
                let message = format!("dependency cycle detected in {phase} phase task graph");
                log.error(&message);
                anyhow::bail!(message);
            }
            crate::engine::scheduler::run_tasks_parallel(&phase_tasks, ctx, log);
        } else {
            for task in &phase_tasks {
                if ctx.is_cancelled() {
                    log.warn("cancelled - stopping before next task");
                    break;
                }
                phases::execute(*task, ctx);
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
