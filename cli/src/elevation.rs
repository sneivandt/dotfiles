//! Windows privilege elevation.
//!
//! Detects whether the process has administrator privileges and re-launches
//! elevated via UAC when needed. No-op on non-Windows platforms.

/// Check if the current process is running with administrator privileges.
///
/// On Windows, attempts to open `HKLM\SOFTWARE` with write access, which
/// only succeeds for elevated processes. On non-Windows platforms, always
/// returns `false`.
#[cfg(windows)]
#[must_use]
pub fn is_elevated() -> bool {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    // KEY_WRITE = 0x20006 — opening HKLM\SOFTWARE with write access
    // succeeds only when the process token is elevated.
    hklm.open_subkey_with_flags("SOFTWARE", 0x20006).is_ok()
}

/// Check if the current process is running with administrator privileges.
///
/// Always returns `false` on non-Windows platforms.
#[cfg(not(windows))]
#[must_use]
pub const fn is_elevated() -> bool {
    false
}

/// Re-launch the current process with administrator privileges via UAC.
///
/// Uses PowerShell `Start-Process -Verb RunAs` to trigger the UAC prompt.
/// On success, an elevated window opens and the current process exits.
///
/// # Errors
///
/// Returns an error if the user cancels the UAC prompt or the elevated
/// process fails to start.
#[cfg(windows)]
pub fn elevate_and_exit() -> anyhow::Result<()> {
    use anyhow::{Context, bail};

    let exe = std::env::current_exe().context("failed to determine current executable path")?;
    let args: Vec<String> = std::env::args().skip(1).collect();

    let exe_escaped = exe.display().to_string().replace('\'', "''");
    let ps_cmd = if args.is_empty() {
        format!("Start-Process -FilePath '{exe_escaped}' -Verb RunAs")
    } else {
        let arg_list = args
            .iter()
            .map(|a| format!("'{}'", a.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(",");
        format!("Start-Process -FilePath '{exe_escaped}' -ArgumentList {arg_list} -Verb RunAs")
    };

    let ps_exe = if crate::exec::which("pwsh") {
        "pwsh"
    } else {
        "powershell"
    };

    eprintln!("Not running as administrator. Requesting elevation...");

    let status = std::process::Command::new(ps_exe)
        .args(["-NoProfile", "-Command", &ps_cmd])
        .status()
        .context("failed to start elevated process")?;

    if status.success() {
        eprintln!("Elevated window opened.");
        std::process::exit(0);
    }

    bail!(
        "UAC elevation was cancelled or failed. \
         Administrator privileges are required. Use --dry-run to preview changes."
    );
}

/// Pause before exiting so the user can read output in an elevated window.
///
/// On Windows, if the process is elevated, prints a prompt and waits for
/// the user to press Enter. No-op on non-Windows or non-elevated processes.
#[cfg(windows)]
pub fn wait_if_elevated() {
    if is_elevated() {
        eprintln!();
        eprint!("Press Enter to close...");
        std::io::stdin().read_line(&mut String::new()).ok(); // Best-effort: ignore read errors
    }
}

/// No-op on non-Windows platforms.
#[cfg(not(windows))]
pub const fn wait_if_elevated() {}
