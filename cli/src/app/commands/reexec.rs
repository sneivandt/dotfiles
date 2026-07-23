//! Application self-update re-execution policy.

use anyhow::Result;

use crate::app::cli::GlobalOpts;
use crate::infra::logging::Output;

use super::runner;

/// Environment variable set before re-exec to prevent infinite self-update loops.
pub(super) const REEXEC_GUARD_VAR: &str = "DOTFILES_REEXEC_GUARD";

/// Exit code used on Windows after staging a self-update so the restart helper
/// knows the binary exited intentionally.
#[cfg(windows)]
const WINDOWS_RESTART_EXIT_CODE: i32 = 75;

/// Replace the current process with a fresh invocation of the same binary.
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
            Ok(status) => {
                if status.code().is_none() {
                    log.warn("child process terminated by signal");
                }
                std::process::exit(status.code().unwrap_or(1))
            }
            Err(error) => {
                log.error(&format!("failed to re-exec: {error}"));
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
pub(crate) fn prepare_self_update(global: &GlobalOpts, log: &dyn Output) -> Result<()> {
    let root = runner::resolve_root(global)?;
    if std::env::var_os(REEXEC_GUARD_VAR).is_none()
        && crate::domains::dotfiles::self_update::pre_update(&root, log, global.dry_run)?
    {
        re_exec(&root, log);
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn re_exec_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join("bin").join("dotfiles")
}

#[cfg(not(unix))]
#[cfg_attr(windows, allow(dead_code, reason = "used conditionally via cfg"))]
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

    let mut command = std::process::Command::new(crate::infra::elevation::preferred_powershell());
    crate::infra::exec::windows::PowerShellCommand::new(&helper_script).configure(&mut command);
    command.spawn().context("spawning restart helper")?;

    Ok(())
}

#[cfg(windows)]
pub(super) fn build_windows_restart_helper_script(
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
        exe = crate::infra::exec::windows::powershell_single_quote(&exe.display().to_string()),
        pending =
            crate::infra::exec::windows::powershell_single_quote(&pending.display().to_string()),
        pending_version = crate::infra::exec::windows::powershell_single_quote(
            &pending_version.display().to_string()
        ),
        cache = crate::infra::exec::windows::powershell_single_quote(&cache.display().to_string()),
        args = crate::infra::exec::windows::powershell_arg_list(args),
        guard = REEXEC_GUARD_VAR,
    )
}
