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
use crate::logging::{Log, Logger, Output};
use crate::platform::Platform;
use crate::tasks::{self, Context, Task};

/// Environment variable set before re-exec to prevent infinite self-update loops.
const REEXEC_GUARD_VAR: &str = "DOTFILES_REEXEC_GUARD";

/// Environment variable set by the `PowerShell` wrapper when it can restart the
/// binary after a staged Windows self-update.
#[cfg(windows)]
const WRAPPER_RESTART_ENV_VAR: &str = "DOTFILES_WRAPPER_RESTART";

/// Exit code used on Windows to ask the wrapper to relaunch after a staged update.
///
/// Keep this in sync with `dotfiles.ps1`.
#[cfg(windows)]
pub(crate) const WINDOWS_RESTART_EXIT_CODE: i32 = 75;

/// Replace the current process with a fresh invocation of the same binary.
///
/// Called after a self-update has replaced the binary on disk so that the
/// new version runs all tasks with updated code.  Sets [`REEXEC_GUARD_VAR`]
/// so the new process skips the self-update step.
#[allow(unused_variables)]
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
        if std::env::var_os(WRAPPER_RESTART_ENV_VAR).is_some() {
            std::process::exit(WINDOWS_RESTART_EXIT_CODE);
        }

        if let Err(err) = spawn_windows_restart_helper() {
            eprintln!("\x1b[31mError: failed to schedule Windows restart: {err}\x1b[0m");
            std::process::exit(1);
        }

        eprintln!(
            "\x1b[33mInfo: update staged; relaunching without wrapper support (exit code {WINDOWS_RESTART_EXIT_CODE})\x1b[0m"
        );
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
        .args(["-NoProfile", "-Command", &helper_script])
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
                         if (Test-Path $exe) {{ Remove-Item $exe -Force }}; \
                         Move-Item -Path $pending -Destination $exe -Force; \
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
#[allow(clippy::unwrap_used)]
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
}

#[cfg(all(test, unix))]
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
    use crate::tasks::{
        TaskResult, task_deps,
        test_helpers::{empty_config, make_static_context},
    };
    use anyhow::Result;
    use std::path::PathBuf;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    struct CycleTaskA {
        ran: Arc<AtomicBool>,
    }

    impl Task for CycleTaskA {
        fn name(&self) -> &'static str {
            "cycle-a"
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
    pub fn new(global: &GlobalOpts, log: &Arc<Logger>) -> Result<Self> {
        let platform = Platform::detect();
        let root = install::resolve_root(global)?;
        let profile = resolve_profile(global, &root, platform, &**log)?;
        let config = load_config(&root, &profile, platform, &**log)?;

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
    platform: Platform,
    log: &dyn Output,
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
    platform: Platform,
    log: &dyn Output,
) -> Result<Config> {
    log.stage("Loading configuration");
    let config = Config::load(root, profile, platform)?;

    let debug_count = |count: usize, label: &str| log.debug(&format!("{count} {label}"));

    debug_count(config.packages.len(), "packages");
    debug_count(config.symlinks.len(), "symlinks");
    debug_count(config.registry.len(), "registry entries");
    debug_count(config.units.len(), "systemd units");
    debug_count(config.chmod.len(), "chmod entries");
    debug_count(config.vscode_extensions.len(), "vscode extensions");
    debug_count(config.copilot_skills.len(), "copilot skills");
    debug_count(config.manifest.excluded_files.len(), "manifest exclusions");
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
/// Returns an error if the task graph contains a dependency cycle or if one or
/// more tasks recorded a failure.
pub fn run_tasks_to_completion<'a>(
    tasks: impl IntoIterator<Item = &'a dyn Task>,
    ctx: &Context,
    log: &Arc<Logger>,
) -> Result<()> {
    let tasks: Vec<&dyn Task> = tasks.into_iter().collect();

    if ctx.parallel && tasks.len() > 1 {
        if tasks::has_cycle(&tasks) {
            let message = "dependency cycle detected in task graph";
            log.error(message);
            anyhow::bail!(message);
        }
        scheduler::run_tasks_parallel(&tasks, ctx, log);
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
